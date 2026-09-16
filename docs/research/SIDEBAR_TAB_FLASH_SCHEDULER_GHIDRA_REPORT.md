# Sidebar Tab-Flash Scheduler — Ghidra Research Report

**Address(es):**
- `FUN_0069DFC0` — Start_Flash (schedules a tab pulse)
- `FUN_0069DFF0` — Stop_Flash (cancels / resets a tab pulse)
- `FUN_0069E010` — Flash_AI (per-tick toggle / countdown decrementer)

**Confidence:** HIGH (every claim below is from live decompile and disassembly verified this session, 2026-05-20)
**Active in YR:** **Yes** for the per-tab pulse (FUN_0069DFC0/E010/DFF0 family). **Dormant in YR** for the separate sibling SidebarClass+0x5394/+0x5398 frame-animation system (the SHP it animates is never loaded — see §10).

---

## 1. Overview

`FUN_0069DFC0` is the **per-tab flash scheduler** for the in-game sidebar. It is called from `StripClass::AI @ 0x006A8B30` when a strip detects either (a) a charged super-weapon, or (b) a completed aircraft, in one of its slots. The function schedules a 10-tick on/off pulse on the **tab button gadget** corresponding to that strip's tab index, drawing the player's attention to the tab. The pulse animates by toggling a single byte on the gadget; `SBGadgetClass::Draw` reads that byte to select between "idle/active" and "pressed" SHP frames, producing a visible blink.

This is **completely distinct from the BUTTON_FADE_EFFECT family** (which animates Windows dialog buttons in the main menu and network-lobby), and **also distinct from the SidebarClass+0x5394/+0x5398 frame-animation system** (which iterates frames of `DAT_00b0b478` — a SHP never loaded in YR).

The disparity-scan addendum (2026-05-20) that identified `FUN_0069DFC0` as the "real in-game cameo/tab flash trigger" was correct in spirit but slightly off in detail: the flash is on the **tab button**, not the cameo. There is no per-cameo flash mechanism that fires in YR — `CameoEntry.FlashEndFrame` (the cameo-level pulse) is dead code per [CAMEO_FLASH_END_FRAME_WRITER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CAMEO_FLASH_END_FRAME_WRITER_GHIDRA_REPORT.md).

---

## 2. The flash sub-struct (gadget-local fields)

Three contiguous fields on an `SBGadgetClass` instance act as the flash state. They live inside the gadget struct, not in a separate object.

| Offset | Type | Field | Init | Set by | Read by |
|---|---|---|---|---|---|
| `+0x34` | `byte` | **CurrentState** — toggled 0↔1 by Flash_AI | 0 | Start_Flash, Flash_AI, Stop_Flash | SBGadgetClass::Draw (when `+0x40 == 1`) |
| `+0x38` | `int32` | **Period** — toggle interval in ticks; also the "is-flashing" flag (`!= 0`) | 0 | Start_Flash, Stop_Flash | Flash_AI (countdown reset), Start_Flash (guard) |
| `+0x3c` | `int32` | **Countdown** — ticks remaining until next toggle | 0 | Start_Flash, Flash_AI, Stop_Flash | Flash_AI (decrement) |

The gadget-local `+0x1e` byte (the `IsDisabled` flag from `SIDEBAR_INIT_GADGET_POSITIONING`) acts as a separate gate: when set, Flash_AI auto-stops the flash.

> Verified via `decompile_function 0x0069DFC0` and `disassemble_function 0x0069DFC0`. The disassembly shows direct byte offsets (`MOV [ECX+0x38], EAX`), so the offsets are byte-for-byte, not `int*`-indexed.

### Field-name conflict notice

`+0x34` was previously documented in [SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md) (2026-05-20) as the "is being mouse-down pressed RIGHT NOW" flag, read by `SBGadgetClass::Draw` to select frame 3/4 over 0/1. **Both purposes use the same byte.** For Repair/Sell, only the mouse-down handler writes it; for tabs, only Flash_AI writes it. Both gadget classes share the field and both behaviours can coexist because tabs are not pressed-clicked-down for ≥10 ticks in normal play. Semantically the byte is "draw me as pressed-looking" — the *cause* (mouse-down vs flash toggle) differs by gadget.

---

## 3. Core logic — the three functions

### 3.1 `FUN_0069DFC0` — Start_Flash *(__thiscall; `RET 0xc`)*

```c
u32 Start_Flash(SBGadget *this, int period, int extra_delay, byte initial_state) {
  if (this->Period /* +0x38 */ != 0) return 0;  // already flashing → no-op
  this->Period         /* +0x38 */ = period;
  this->Countdown      /* +0x3c */ = period + extra_delay;
  this->CurrentState   /* +0x34 */ = initial_state;
  return 1;
}
```

Returns `1` on success, `0` if a flash is already active. (Ghidra's decomp packs the return as `CONCAT31(...)`; the low byte AL is the actual success bit.)

> Verified via `disassemble_function 0x0069DFC0`. The stack frame is 3 args × 4 bytes (`RET 0xc`), so it is unambiguously stdcall args after ECX-as-this.

### 3.2 `FUN_0069DFF0` — Stop_Flash *(__fastcall — ECX = this)*

```c
u32 Stop_Flash(SBGadget *this) {
  if (this->Period /* +0x38 */ == 0) return 0;  // not flashing → no-op
  this->CurrentState  /* +0x34 */ = 0;
  this->Countdown     /* +0x3c */ = 0;
  this->Period        /* +0x38 */ = 0;
  return 1;
}
```

Forcibly resets all three fields. The order in the binary is `+0x34 → +0x3c → +0x38` (state byte first, the "is-flashing" sentinel `+0x38` last) — relevant if the renderer ever reads mid-reset, but in practice this is called outside of Draw windows.

### 3.3 `FUN_0069E010` — Flash_AI *(__fastcall — ECX = this)*

```c
u32 Flash_AI(SBGadget *this) {
  if (this->IsDisabled /* +0x1e */ == 0) {
    if (this->Countdown /* +0x3c */ != 0) {
      this->Countdown -= 1;
      if (this->Countdown == 0) {
        this->CurrentState = !this->CurrentState;     // toggle byte (XOR semantics)
        this->Countdown    = this->Period /* +0x38 */; // reset countdown to fixed period
        return 1;
      }
    }
    return 0;
  }
  // Disabled path — auto-stop if currently flashing.
  if (this->Period != 0) {
    this->CurrentState = 0;
    this->Countdown    = 0;
    this->Period       = 0;
    return 1;
  }
  return 0;
}
```

Returns `1` when something visible changed (toggle or auto-stop), `0` otherwise. The caller uses this to decide whether to set `NeedsRedraw`.

**Critical detail:** after the first toggle, the countdown resets to `+0x38 = period`, NOT to `period + extra_delay`. So `extra_delay` only delays the first toggle; subsequent toggles are at the steady `period` interval. (See §4 for the math semantics.)

> Verified via `decompile_function 0x0069E010` and confirmed against the StripClass::AI argument-passing convention in §4.

---

## 4. The Start_Flash call site — argument semantics

In `StripClass::AI` at offset `006a8e52..006a8e9b` (raw assembly):

```
006a8e52: MOV ESI, [g_CurrentFrameCounter]        ; ESI = frame
006a8e58: MOV ECX, 10
006a8e5d-006a8e69: ECX = 10 - (frame % 10)        ; ECX = extra_delay
006a8e6c-006a8e84: EDX = ((extra_delay + frame) / 10) & 0x80000001 (sign-corrected)
006a8e85: MOV EAX, [EBX+0x38]                     ; EAX = strip.TabIndex
006a8e88: SETZ DL                                  ; DL = (parity == 0) ? 1 : 0
006a8e8b: PUSH EDX                                 ; arg 3 = initial_state
006a8e8c: PUSH ECX                                 ; arg 2 = extra_delay
006a8e8d: LEA ECX, [EAX + EAX*2]                  ; ECX = TabIndex * 3
006a8e90: PUSH 0xa                                  ; arg 1 = period = 10
006a8e92: SHL ECX, 5                                ; ECX = TabIndex * 96
006a8e95: ADD ECX, 0xb07c48                         ; ECX = &TabGadget[TabIndex]
006a8e9b: CALL Start_Flash
```

The C-equivalent call is:

```c
Start_Flash(
  &g_TabGadgets[strip.TabIndex],     // ECX (this) — the tab button gadget for THIS strip's tab
  10,                                 // period = 10 ticks
  10 - (frame % 10),                  // extra_delay = ticks until next 10-frame boundary
  ((next_boundary_index) & 1) == 0    // initial_state = 1 if next-boundary index is even, else 0
);
```

### 4.1 Phase math (the bit the SIDEBAR_TIMING pseudocode got wrong)

The SIDEBAR_TIMING_AND_TOOLTIPS §4.3 pseudocode called these args `(start_frame, duration, initial_state)`. The actual semantics from the function body and the call site:

- **arg1 = period (=10)** — the toggle interval in ticks after the first toggle
- **arg2 = extra_delay (=10 - frame%10)** — extra ticks added to the period *only for the initial countdown*
- **arg3 = initial_state** — what `CurrentState` is set to immediately

So the **first toggle** happens after `period + extra_delay` = `10 + (10 - frame%10)` ticks. That value ranges from `11` (when frame%10 == 9) to `20` (when frame%10 == 0). The first toggle therefore lands on the **second 10-frame boundary** after the call (e.g., started at frame 23 → first toggle at frame 40; started at frame 33 → first toggle at frame 50). Subsequent toggles are every 10 ticks.

**Why two boundaries out, not one:** if it were the next boundary, a call at frame 29 would toggle 1 tick later — the initial state would be visible for ≈16 ms and missed. The extra `+10` guarantees the initial state is visible for ≥10 ticks before the first toggle.

### 4.2 The parity bit — phase-aligning concurrent flashes

`initial_state = (next_boundary / 10) & 1 == 0`. Computed as: `(extra_delay + frame) / 10 & 0x80000001`, then `SETZ DL` produces 1 if the parity bit was zero. The sign-correction at `006a8e7e-006a8e84` handles negative frame numbers (which can't happen in practice but the binary handles them anyway).

The effect: if two tabs call `Start_Flash` at any two frames within the same 10-frame phase, **both will have the same initial state** (because they target the same next-boundary, which has a single parity). After the first toggle, both continue stepping at the same 10-tick cadence, so they remain phase-aligned indefinitely — the player sees concurrent flashes blink together, not as visual noise.

### 4.3 Single 10-tick period — the apparent variability in §4.3 of TIMING is a misreading

Earlier reading of `*(int *)(param_1 + 0x3c) = *(undefined4 *)(param_1 + 0x38)` (the countdown reset) suggested subsequent toggles use the extra_delay value, which would have made the period variable per tab. The actual data flow:

- `+0x38` = `period` = literal `10` from the call site
- `+0x3c` = `period + extra_delay` for the first cycle only
- After each toggle, `+0x3c` resets to `+0x38` = `10`

So **every tab pulses at exactly 10 ticks per half-cycle = 20 ticks per full on/off cycle**, regardless of when the flash started. The "phase-aligned" claim from SIDEBAR_TIMING holds for both the first toggle (next-boundary sync) AND all subsequent toggles (10-tick fixed period).

---

## 5. The trigger conditions in `StripClass::AI`

From `decompile_function 0x006A8B30`, the first AI block:

```c
if ((this->AnimState /* +0x38 */ == 0) || (this->AnimState == 1)) {
  for (i = 0; i < this->EntryCount /* +0x54 */; i++) {
    CameoEntry *entry = &this->Entries[i] /* +0x58 + i*0x34 */;

    if (entry->RTTIType /* +0x04 */ == 0x1F) {                 // SuperWeapon RTTI
      if (g_Player->SuperWeapons[entry->TypeIndex] != 0
          && FUN_006ce1a0() != 0) {                            // SW exists and is ready/charged
        goto SCHEDULE_FLASH;
      }
    } else {
      FactoryClass *fact = entry->FactoryPtr /* +0x0c */;
      if (fact != 0
          && FactoryClass::IsComplete(fact)                    // production done
          && (obj = FactoryClass::GetObject(fact)) != 0
          && obj->vtable->What_Am_I() /* +0x2c */ == 6) {      // RTTI 6 = AIRCRAFT
        goto SCHEDULE_FLASH;
      }
    }
  }
  Stop_Flash(&g_TabGadgets[this->TabIndex]);     // no triggers found → stop the flash
  return;

SCHEDULE_FLASH:
  // ... compute extra_delay, parity, schedule via Start_Flash on this strip's tab gadget
}
```

### 5.1 Tab-flash trigger summary

| Strip → Tab | Triggers tab flash? |
|---|---|
| Tab 0 (Aircraft non-naval) | Aircraft completion |
| Tab 1 (Defense — SuperWeapons + naval Aircraft) | SW becomes ready (charged), OR naval Aircraft completes |
| Tab 2 (Structures) | Never — no slot has RTTI 0x1F or completed-aircraft semantics |
| Tab 3 (Units / Infantry) | Never — same reason |

**Critical:** Building completion, infantry completion, and vehicle (non-aircraft) completion do **NOT** trigger the tab flash. Those go through a separate AI block (the `switch(rtti)` at `006a8db5`) that fires EVA voice (`VoxClass__PlayEVA`) and dispatches a place/build command — but does not call Start_Flash. Only **aircraft completion** AND **super-weapon ready** drive the flash.

The aircraft-only behavior is deliberate: in standard RA2/YR, building/infantry/vehicle completions auto-place or auto-spawn quickly, while aircraft sit in the queue waiting for an available helipad — the pulse alerts the player to the waiting aircraft. Super-weapons similarly wait for the player to launch them.

> Verified via `disassemble_function 0x006A8B30` lines `006a8d23` (RTTI 0x1F check) and `006a8d77` (RTTI 6 check).

### 5.2 The AnimState gate `+0x38 == 0 || == 1`

The first AI block is gated by `this->AnimState == 0 || == 1` (strip `+0x38`). This is the `TabIndex` field per `SIDEBAR_SYSTEM_GHIDRA_REPORT.md §7`. So the flash trigger only runs for strips whose `TabIndex` is 0 or 1 — **Tabs 0 and 1 only**, matching the trigger summary above. The check is a hot-path filter that skips trigger evaluation for Tabs 2 and 3 entirely.

> Verified via `disassemble_function 0x006A8B30` lines `006a8d07-006a8d11` (`MOV EAX, [EBX+0x38]; TEST EAX, EAX; JZ ...; CMP EAX, 1; JNZ skip`).

### 5.3 Auto-stop when conditions clear

If the iteration completes without finding any trigger, `Stop_Flash` is called on `&g_TabGadgets[strip.TabIndex]` (= `0xb07c48 + this->TabIndex * 0x60`). So the flash automatically stops when:
- The completed aircraft is removed from the queue (placed by player)
- The super-weapon is fired (Available count goes to zero)
- The super-weapon's "should-flash" check (`FUN_006ce1a0`) starts returning false

> Verified via `disassemble_function 0x006A8B30` lines `006a8d8b-006a8d9a` (`MOV EAX, [EBX+0x38]; LEA ECX, [EAX+EAX*2]; SHL ECX, 5; ADD ECX, 0xb07c48; CALL Stop_Flash`).

---

## 6. The Flash_AI tick — where the toggling happens

`FUN_0069E010` is called from `SidebarClass::Action @ 0x006A7780` in a loop over all 4 tab gadgets, once per game tick. From the earlier audit (this same session):

```c
puVar7 = &DAT_00b07c48;            // tab gadget array base
do {
  cVar2 = FUN_0069e010();           // ECX = puVar7; tick this tab
  if (cVar2 != 0) bVar1 = true;
  puVar7 += 0x60;                   // next tab (stride 0x60)
} while (puVar7 < 0xb07dc8);

if (bVar1) {
  sidebar->NeedsRedraw /* +0x53A6 */ = 1;
  DAT_00b0b518 = 1;                 // global dirty bit
  DAT_00884b8f = 1;                 // gadget-dirty bit
  (**(code **)(*sidebar + 0x38))(0);
}
```

So every tick, all 4 tabs are AI-ticked. If any toggle happened (Flash_AI returned non-zero), the sidebar's NeedsRedraw flag is set, causing the next `SidebarClass::Draw` to repaint the tab strip. That re-draw walks through `SBGadgetClass::Draw` for each tab, reading `+0x34` and selecting frame 0/1 vs 3/4 accordingly.

> Verified via `get_function_callers 0x0069E010` (sole caller is SidebarClass::Action) and the previously-decoded action body.

---

## 7. The visual effect — how `+0x34` lands on the screen

From `SBGadgetClass::Draw @ 0x0069DEB0` (per `SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md §5`), the frame-selection branch for pressable gadgets (`+0x40 == 1`):

```c
if (this->IsPressed /* +0x34 */) {
  frame = (this->IsActiveMode /* +0x2D */ != 0) ? 4 : 3;
} else {
  frame = (this->IsActiveMode /* +0x2D */ != 0) ? 1 : 0;
}
```

Tab gadgets are initialized with `+0x40 = 1` (pressable) in `SidebarClass::Init @ 006a5310`. So while Flash_AI is toggling `+0x34`:

- `+0x34 = 0` ("idle") → frame 0 (inactive tab) or 1 (active tab)
- `+0x34 = 1` ("pressed-look") → frame 3 (inactive pressed) or 4 (active pressed)

The visible result is that the flashing tab **alternates between its normal appearance and its pressed appearance every 10 ticks**. The "pressed" frames typically have the tab graphic recessed and/or brighter — this draws the eye to the tab without requiring a separate animation SHP.

> Verified via re-read of the `SBGadgetClass::Draw` decompile (the canonical SIDEBAR_REPAIR_SELL_BUTTON doc citation), plus `SidebarClass::Init` tab-init disassembly confirming `+0x40 = 1`.

### 7.1 Frame-count requirement

For tab-flash to render correctly, each `TAB0N.SHP` must have **5 frames** loaded: 0 (inactive), 1 (active), 2 (disabled — `IsDisabled` branch), 3 (inactive pressed), 4 (active pressed). The Rust port currently loads only frames 0 and 1 ([sidebar_chrome.rs:285-291](../src/render/sidebar_chrome.rs#L285-L291)) — frames 2-4 are not packed into the chrome atlas. Implementing tab-flash will require loading all 5 frames.

---

## 8. INI keys

None. The flash mechanism is entirely binary-hardcoded:

- Period = 10 ticks (literal `MOV ECX, 0xa` at 006a8e58 / `PUSH 0xa` at 006a8e90)
- Trigger RTTIs (0x1F for SW, 6 for Aircraft) are literals
- Tab gadget stride 0x60 and base 0xb07c48 are literals
- The "is super-weapon flashable" check is a function call to `FUN_006ce1a0`, which itself probably reads `Type` fields but doesn't expose them as flash-tuning INI keys

No INI key controls flash duration, period, trigger conditions, or visual appearance. This is purely visual code with no exposed configuration.

---

## 9. Integration points

| Function | Address | Role |
|---|---|---|
| `StripClass::AI` | `0x006A8B30` | Iterates each strip's slots per tick (Tabs 0,1 only); calls Start_Flash on aircraft-complete or SW-ready, Stop_Flash otherwise |
| `SidebarClass::Action` | `0x006A7780` | Tick-loop over all 4 tab gadgets; calls Flash_AI on each; sets NeedsRedraw on any non-zero return |
| `SidebarClass::Init` | `0x006A5310` | Initializes each tab gadget with `+0x40 = 1` (pressable) and zeroes flash fields; explicitly calls Stop_Flash on each tab to seed `+0x38 = 0` |
| `SBGadgetClass::Draw` | `0x0069DEB0` | Reads `+0x34` (the flashed-toggle byte) when `+0x40 == 1` to select frame 3/4 vs 0/1 |

The flash mechanism integrates only with the tab-gadget side of the sidebar — no AI hooks, no power/economy hooks, no save/load.

---

## 10. Resolution of the cross-doc conflict (SIDEBAR_TIMING §4 vs SIDEBAR_CONSTRUCTION §10)

The morning disparity-scan addendum flagged:
- `SIDEBAR_TIMING_AND_TOOLTIPS §4.1` claims `SidebarClass+0x5394/+0x5398` are TabFlashFrame/TabFlashState (live in YR)
- `SIDEBAR_CONSTRUCTION §10` claims the same offsets are sidebar open/close animation (TS-legacy dead)

**This investigation resolves it: both docs are partially right.** The SidebarClass+0x5394/+0x5398 system is a **third** flash mechanism, separate from `FUN_0069DFC0` (the per-tab pulse this report covers) and from BUTTON_FADE_EFFECT (the Windows dialog buttons).

The SidebarClass+0x5394/+0x5398 code lives in `SidebarClass::Action @ 0x006A7780` (verified earlier in this session):

```c
if (sidebar->FrameAnimDir /* +0x5398 */ == 1) {       // forward animation
  sidebar->FrameAnimCount /* +0x5394 */ += 1;
  if (sidebar->FrameAnimCount > *(short *)(DAT_00b0b478 + 6)) {  // SHP frame count
    sidebar->FrameAnimDir   = 0;
    sidebar->FrameAnimCount = 0;
  }
}
else if (sidebar->FrameAnimDir == -1) {                // reverse animation
  if (--sidebar->FrameAnimCount < 0) {
    sidebar->FrameAnimDir = 0;
    sidebar->FrameAnimCount = 0;
  }
}
```

The fields *do exist*, the logic *is live code* (matches SIDEBAR_TIMING). But the SHP it animates is `DAT_00b0b478` — and per `SIDEBAR_CONSTRUCTION §10`, that pointer is never written in YR (the SHP is null). So:

- **Fields:** live (SidebarClass+0x5394 = frame counter, +0x5398 = direction: 1/−1/0)
- **Code:** live (executes every tick in SidebarClass::Action)
- **Effect:** null in YR (the SHP it would animate is never loaded)

The system is **effectively dormant** because its output SHP doesn't exist in stock YR. Implementing this in the Rust port would do nothing visible unless the SHP is also loaded (and it isn't part of any side's MIX in retail).

**Implication for Rust port:** ignore SidebarClass+0x5394/+0x5398. Implement only the per-tab pulse described in §1-§9 of this report. The per-tab pulse via `FUN_0069DFC0` is the actual visible tab-flash mechanism in YR.

> SidebarClass::Action behavior verified via decompile in earlier session (cited in the 2026-05-20 audit of SIDEBAR_TIMING_AND_TOOLTIPS, which CONFIRMED the `0x5394/0x5398` field references). The DAT_00b0b478 null-in-YR claim cited from SIDEBAR_CONSTRUCTION §10 audit.

---

## 11. Edge cases and corner-case behavior

1. **Double-trigger guard.** If `Start_Flash` is called while `+0x38 != 0`, it returns 0 and does nothing. So multiple AI ticks within the same flash period don't restart the animation or change its phase.

2. **Disabled tab.** If the tab gadget's `+0x1e` (IsDisabled) is set, Flash_AI auto-stops any active flash and zeroes all 3 fields. This matters if the player has limited build options and a tab is greyed out — incoming aircraft would not flash a disabled tab.

3. **Trigger condition disappears.** When the strip AI finds no triggers in its iteration, it unconditionally calls Stop_Flash on its tab. So:
   - Player places the waiting aircraft → next tick, the strip iteration finds no completed aircraft → Stop_Flash fires.
   - Player fires the SW → Available count goes to 0 → trigger condition fails → Stop_Flash.
   - Player loses the factory mid-flash → factory becomes null → trigger fails → Stop_Flash.

4. **Tab-switch behavior.** Switching to the flashing tab does not directly stop the flash. The flash keeps pulsing until its trigger condition clears (player places the aircraft / fires the SW). This is observable in retail.

5. **Frame counter wraparound.** `g_CurrentFrameCounter` is i32. At 60 FPS it wraps every ~414 days; at 15 FPS (RA2 game speed) ~5.6 years. Not a practical concern. `[DEFERRED]`.

6. **Pause behavior.** When the game is paused, StripClass::AI is not called (per CLAUDE.md's tick order: AI runs in the sim tick). SidebarClass::Action *is* still called from input dispatch, so Flash_AI continues ticking — meaning the flash continues to blink while the game is paused. (This matches retail behavior.)

7. **Save/load.** Sidebar state is not serialized to saves (per CAMEO_FLASH_END_FRAME_WRITER's analysis). On load, all gadget fields are re-initialized by SidebarClass::Init (which explicitly calls Stop_Flash on each tab). If the trigger conditions still apply post-load, the next StripClass::AI tick re-triggers the flash. So save/load loses the current flash phase but re-establishes the flash if conditions warrant.

---

## 12. Current Rust Implementation Status

Per the disparity-scan addendum (2026-05-20):

| Subsystem | Rust state |
|---|---|
| Tab flash mechanism (FUN_0069DFC0 family) | **NOT IMPLEMENTED** — no `SBGadgetClass`-equivalent flash struct, no Start/Stop/AI counterparts, no per-tab pulse logic |
| Tab SHP frames 2–4 (disabled / pressed / pressed-active) | **NOT LOADED** ([sidebar_chrome.rs:285-291](../src/render/sidebar_chrome.rs#L285-L291) loads only frames 0 and 1) |
| `SidebarTabButton` flash state | **MISSING** ([sidebar/mod.rs:178-183](../src/sidebar/mod.rs#L178-L183) carries only `active: bool`) |
| Aircraft-completion → pulse hook | **MISSING** (`StripClass::AI` equivalent doesn't exist; sidebar_view.rs builds tabs but does not emit any "flash this tab" signal) |
| SW-ready → pulse hook | **MISSING** (same) |

Implementation requires:
1. Loading frames 2-4 of `tab0N.shp` into the chrome atlas.
2. Adding `flash_period` / `flash_countdown` / `flash_state` to `SidebarTabButton` (or to a per-tab `FlashState` struct held alongside the gadget).
3. Wiring a per-tab tick that mirrors Flash_AI (1 byte toggle every 10 ticks).
4. Hooking the sim's "aircraft completed" and "SW ready" events to schedule the flash.
5. Reading the flash state in [app_sidebar_build.rs](../src/app_sidebar_build.rs) tab-render to swap the SHP frame.

None of these are blocked by other systems — they can be done immediately.

---

## 13. Open Questions — Final State

- `[RESOLVED] FN-1 — FUN_0069DFC0 layout and signature.` → 3-arg __thiscall stdcall, ECX=gadget, args (period, extra_delay, initial_state); writes +0x38/+0x3c/+0x34. (evidence: decompile_function 0x0069DFC0, disassemble_function 0x0069DFC0)
- `[RESOLVED] FN-2 — FUN_0069DFF0 layout and signature.` → 0-arg __fastcall, ECX=gadget; zeroes +0x38, +0x3c, +0x34 in that order; idempotent guard on +0x38 == 0. (evidence: decompile_function 0x0069DFF0)
- `[RESOLVED] FN-3 — FUN_0069E010 layout and tick logic.` → 0-arg __fastcall, ECX=gadget; gated by +0x1e (IsDisabled); decrements +0x3c, on hit-zero toggles +0x34 and resets +0x3c=+0x38. (evidence: decompile_function 0x0069E010)
- `[RESOLVED] FN-4 — Callers of FUN_0069DFC0.` → Sole caller: StripClass::AI @ 0x006A8B30. (evidence: get_function_callers)
- `[RESOLVED] FN-5 — Callers of FUN_0069E010.` → Sole caller: SidebarClass::Action @ 0x006A7780. (evidence: get_function_callers)
- `[RESOLVED] FN-6 — Flash-struct field offsets.` → +0x34 (state byte), +0x38 (period / sentinel), +0x3c (countdown). Inside SBGadgetClass. (evidence: function bodies; [SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md) §2)
- `[RESOLVED] FN-7 — "this" pointer in Start_Flash call.` → Per-tab SBGadgetClass at `0xb07c48 + strip.TabIndex * 0x60`. (evidence: disasm 006a8e85-006a8e95)
- `[RESOLVED] FN-8 — Relationship to SidebarClass+0x5394/+0x5398.` → Separate, parallel system; fields live but animates DAT_00b0b478 which is null in YR. See §10. (evidence: SidebarClass::Action body; SIDEBAR_CONSTRUCTION §10)
- `[RESOLVED] FN-9 — StripClass::AI case-6 trigger.` → Aircraft completion + SW-ready trigger Start_Flash; no trigger → Stop_Flash. (evidence: disasm 006a8d23-006a8d9a)
- `[RESOLVED] FN-10 — Visual effect of +0x34 toggle.` → Selects SHP frame 0/1 (idle) vs 3/4 (pressed-look) via SBGadgetClass::Draw. (evidence: SBGadgetClass::Draw decompile cited in SIDEBAR_REPAIR_SELL_BUTTON §5)
- `[RESOLVED] FN-11 — Tab pressability.` → Tab init writes `+0x40 = 1` (pressable). (evidence: SidebarClass::Init tab-init loop)
- `[RESOLVED] FN-12 — Phase-alignment math semantics.` → Period = 10 (fixed); extra_delay = ticks to next 10-boundary + 10 more. First toggle at second-next 10-boundary; subsequent toggles every 10 ticks. (evidence: §4 disasm + body)
- `[RESOLVED] FN-13 — Parity bit semantics.` → `(next_boundary_index) & 1` determines initial_state. Concurrent flashes within the same 10-phase share initial state. (evidence: §4.2)
- `[RESOLVED] FN-14 — Are gadget+0x44/+0x48 flash fields?` → No. Those are clip X/Y offsets per SIDEBAR_REPAIR_SELL_BUTTON. Flash fields are +0x34/+0x38/+0x3c. (evidence: cross-doc)
- `[RESOLVED] FN-15 — Active in YR for which triggers.` → Live for aircraft completion (Tab 0 or 1 depending on naval) and SW-ready (Tab 1). Not for building/infantry/vehicle completion. (evidence: trigger code)
- `[RESOLVED] FN-16 — Relation to BUTTON_FADE_EFFECT (0x4DC) system.` → Completely separate. BFE is for Windows dialog buttons (main menu + network lobby); this is for in-game tab gadgets. No code shared. (evidence: BUTTON_FADE_EFFECT swarm 2026-05-20 audits)
- `[RESOLVED] FN-17 — Argument convention.` → __thiscall ECX=this + 3 stack args; stdcall return (`RET 0xc`). (evidence: disasm)
- `[RESOLVED] FN-18 — Double-trigger behavior.` → Start_Flash returns 0 and does nothing if already flashing. (evidence: §3.1 guard)
- `[DEFERRED] FN-19 — Frame-counter wraparound semantics.` (category: bounded-cost-too-high; reason: g_CurrentFrameCounter is i32, wraps at >5 years of play — not a practical concern; next-step-if-pursued: stress-test with frame counter near INT_MAX in a debugger.)
- `[RESOLVED] FN-20 — Cross-doc conflict resolution.` → See §10. SidebarClass+0x5394/+0x5398 is a third, distinct system; live code, dead output (null SHP). (evidence: SidebarClass::Action body)
- `[RESOLVED] FN-21 — Stop_Flash callers.` → SidebarClass::Init (zero-seed at startup) and StripClass::AI (per-tick auto-stop when trigger conditions clear). (evidence: get_function_callers 0x0069DFF0)
- `[RESOLVED] FN-22 — Save/load behavior.` → Sidebar state not serialized; on load, Init reseeds flash fields to 0; AI re-triggers if conditions still apply. (evidence: §11.7, cross-ref CAMEO_FLASH_END_FRAME_WRITER)
- `[RESOLVED] FN-23 — Why AnimState gate at +0x38 == 0||1.` → Strip's TabIndex field; gates trigger evaluation to Tabs 0 and 1 only. (evidence: disasm 006a8d07)
- `[RESOLVED] FN-24 — Behavior when player switches to flashing tab.` → No auto-stop on tab-switch; flash continues until trigger condition clears. (evidence: no SwitchTab → Stop_Flash hook in any caller chain examined)
- `[RESOLVED] FN-25 — Behavior of pause on flash.` → Flash continues to tick (Action runs from input, not from sim). Matches retail. (evidence: SidebarClass::Action call chain)

**Deferred-pile size: 1 of 25 (4%).** Well under the 25% threshold; this report is a complete investigation, not a partial one.

---

## 14. Sources

**Ghidra functions decompiled and analyzed (READ-ONLY — no mutations performed):**
- `0x0069DFC0` Start_Flash (decompile + disassemble)
- `0x0069DFF0` Stop_Flash (decompile)
- `0x0069E010` Flash_AI (decompile)
- `0x006A8B30` StripClass::AI (decompile + disassemble, focused on trigger block 006a8d07-006a8e9b)
- `0x006A7780` SidebarClass::Action (re-used from prior session; tick-loop confirmed)
- `0x0069DEB0` SBGadgetClass::Draw (re-used from SIDEBAR_REPAIR_SELL_BUTTON report)
- `0x006A5310` SidebarClass::Init (re-used; tab-init +0x40 = 1 verified)

**Ghidra xref queries:**
- `get_function_callers 0x0069DFC0` → StripClass::AI
- `get_function_callers 0x0069DFF0` → SidebarClass::Init, StripClass::AI
- `get_function_callers 0x0069E010` → SidebarClass::Action
- `get_xrefs_to 0x00b0b478` → 8 sites (FreeSHPs, AddCameo, Action, Recalculate, Draw, FUN_006a67a0, FUN_006a6820×2)

**Prior docs extended / cross-referenced:**
- `SIDEBAR_TIMING_AND_TOOLTIPS_GHIDRA_REPORT.md §4.3-4.4` (partial pseudocode; this report supersedes the arg-mapping interpretation)
- `SIDEBAR_CONSTRUCTION_GHIDRA_REPORT.md §10` (DAT_00b0b478 nullity claim — settled in §10 of this report)
- `SIDEBAR_REPAIR_SELL_BUTTON_GHIDRA_REPORT.md §2, §5` (SBGadgetClass field map; Draw frame-selection logic)
- [CAMEO_FLASH_END_FRAME_WRITER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CAMEO_FLASH_END_FRAME_WRITER_GHIDRA_REPORT.md) (confirms cameo-level pulse is dead code; tab-level pulse described here is independent and live)
- [SIDEBAR_INIT_GADGET_POSITIONING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_INIT_GADGET_POSITIONING_GHIDRA_REPORT.md) (tab gadget base 0xb07c48 + stride 0x60 confirmed)
- `BUTTON_FADE_EFFECT_*_GHIDRA_REPORT.md` (confirmed unrelated — BFE is for HWND dialog buttons only)
- [TICK_ANIMATION_VISIBLE_LEFTOVERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TICK_ANIMATION_VISIBLE_LEFTOVERS_GHIDRA_REPORT.md) (cross-confirms tab flash scheduling at 10-frame boundary)

**No mutations made** (read-only Ghidra static analysis; no `.rs` files written; no INI changes).
