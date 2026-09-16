# InitSideMixFiles — Ghidra Research Report

**Address:** `0x00534fa0`  
**Confidence:** High (function fully decompiled; all format strings read from memory; callers traced)  
**Active in YR:** Yes — called unconditionally in `ScenarioClass__Full_Init` for every game start (both singleplayer and multiplayer paths)

---

## 1. Overview

`InitSideMixFiles` (at `0x00534fa0`) loads up to three side-specific MIX archives for the
active player's side, then loads UIMD.INI from the primary archive and calls palette and
sidebar-SHP initialization. It is called with a side index (0=Allied, 1=Soviet, 2=Yuri)
derived from `ScenarioClass+0x34B8`, with a hard-coded Yuri→Soviet substitution (`if side==2: side=1`)
so Yuri always loads the Soviet archives. The function returns 1 on success, 0 on failure.

---

## 2. Exact Format Strings (verified from memory reads)

All three strings are contiguous in `.rdata` starting at `0x00827dd4`:

| Address | Format string | Resolved (side 0) | Resolved (side 1/2) |
|---|---|---|---|
| `0x00827dd4` | `SIDENC%02d.MIX` | `SIDENC01.MIX` | `SIDENC02.MIX` |
| `0x00827de4` | `SIDEC%02d.MIX` | `SIDEC01.MIX` | `SIDEC02.MIX` |
| `0x00827e0c` | `SIDEC%02dMD.MIX` | `SIDEC01MD.MIX` | `SIDEC02MD.MIX` |

Memory verification (via `read_memory 0x00827dd4`, length 80):
- Bytes 0–13: `534944454e43253032642e4d49580000` = `SIDENC%02d.MIX\0\0`
- Bytes 16–28: `5349444543253032642e4d495800` = `SIDEC%02d.MIX\0`
- Bytes 32–46 (at +0x38 from start = `0x00827e0c`): `5349444543253032644d442e4d49580` = `SIDEC%02dMD.MIX\0`

Debug string at `0x00827e1c`: `"Preparing Mixfiles for Side %02d.\n"` (confirmed by `search_strings` and `read_memory`).

---

## 3. Core Logic — Step-by-Step

```pseudocode
InitSideMixFiles(side: int) -> bool:

  // Step 0: Call VoxClass__SetSide() unconditionally
  VoxClass__SetSide()

  // Step 1: Yuri→Soviet substitution (CRITICAL)
  if side == 2:
    side = 1

  // Step 2: Handle sentinel value -1 (disk detection path)
  if side == -1:
    side = FUN_004a80d0()   // returns constant 2 — always
    if side != 0 and side != 1:
      log("FAILED! This was not disk one or two!!!")
      return 0

  // Step 3: Log
  log("Preparing Mixfiles for Side %02d.", side)

  // Step 4: Release any previously loaded side MIX archives
  for each global in [DAT_00884e68, DAT_00884e74, DAT_00884e70, DAT_00884e78]:
    if global != NULL:
      log("Releasing %s", global[3])   // global[3] = filename field
      call global->vtable[0](1)        // CDFileClass destructor / unload
      global = NULL

  // Step 5: Increment — param_1 is now side+1 (for %02d = "01", "02")
  side = side + 1

  // Step 6: Open SIDEC%02dMD.MIX (optional — no hard failure if absent)
  filename = sprintf("SIDEC%02dMD.MIX", side)
  log("     Initilizing %s", filename)
  f = CCFileClass::Constructor(filename)
  exists = f->IsAvailable()
  cleanup CCFileClass
  if exists:
    DAT_00884e70 = CDFileClass::Constructor(filename, &DAT_00886980)
  FUN_005b43f0(0)   // stub — returns 1, no side effect verified

  // Step 7: Open SIDEC%02d.MIX (MANDATORY — failure here returns 0)
  filename = sprintf("SIDEC%02d.MIX", side)
  log("     Initilizing %s", filename)
  f = CCFileClass::Constructor(filename)
  exists = f->IsAvailable()
  cleanup CCFileClass
  if NOT exists:
    log("     FAILED!")
    return 0
  if operator_new(0x28) == NULL:
    DAT_00884e74 = NULL
    log("     FAILED!")
    return 0
  DAT_00884e74 = CDFileClass::Constructor(filename, &DAT_00886980)

  // Step 8: Only proceed if SIDEC%02d.MIX was loaded
  if DAT_00884e74 == NULL:
    log("     FAILED!")
    return 0

  FUN_005b43f0(0)   // stub

  // Step 9: Open SIDENC%02d.MIX (optional — no hard failure if absent)
  filename = sprintf("SIDENC%02d.MIX", side)
  log("     Initilizing %s", filename)
  f = CCFileClass::Constructor(filename)
  exists = f->IsAvailable()
  cleanup CCFileClass
  if exists:
    if operator_new(0x28) != NULL:
      DAT_00884e78 = CDFileClass::Constructor(filename, &DAT_00886980)

  // Step 10: Load palettes
  PaletteLoad()

  // Step 11: Set sidebar text color
  SetSidebarTextColor()

  // Step 12: Load UIMD.INI (MANDATORY — failure returns 0)
  f = CCFileClass::Constructor("UIMD.INI")
  ini = SHAPipe::Constructor(...)
  if ini == 0:
    log("Failed to load UIMD.INI!")
    return 0

  // Step 13: Read command bar config from UIMD.INI
  if g_GameMode == 0 or g_GameMode == 5:
    RulesClass::ReadCommandBar(&DAT_00887208, 0)
  else:
    RulesClass::ReadCommandBar(&DAT_00887208, 1)

  // Step 14: Load sidebar SHPs
  FUN_006d02b0()   // calls SidebarClass::LoadSHPs + button art

  cleanup UIMD.INI fileclass

  return 1
```

---

## 4. Archive Load Order (Exact, for side 0 = Allied)

| Step | Archive | Global | Optional? | Notes |
|---|---|---|---|---|
| 1st | `SIDEC01MD.MIX` | `DAT_00884e70` | YES — skipped if not found | YR expansion art |
| 2nd | `SIDEC01.MIX` | `DAT_00884e74` | NO — missing = return 0 | Base allied art; MANDATORY |
| 3rd | `SIDENC01.MIX` | `DAT_00884e78` | YES — skipped if not found | Neutral/shared fallback art; requires SIDEC01.MIX to be loaded first |

SIDENC01.MIX is loaded ONLY if DAT_00884e74 (SIDEC01.MIX) was successfully set.
The `if (DAT_00884e74 != NULL)` guard wraps the entire SIDENC load and all subsequent init.

---

## 5. Yuri→Soviet Substitution

**Exact code (verified, decompiled at `0x00534fa0`):**
```c
if (param_1 == 2) {
    param_1 = 1;
}
```

This substitution happens BEFORE the `side == -1` sentinel check and BEFORE `side += 1`.
Result: Yuri (side 2) loads `SIDEC02MD.MIX`, `SIDEC02.MIX`, `SIDENC02.MIX` — identical to Soviet.

---

## 6. The -1 Sentinel Path (disk detection, Legacy)

```c
if ((param_1 == -1) && (param_1 = FUN_004a80d0(), param_1 != 0)) &&
     (param_1 != 1)) {
  Register_heap_pool("     FAILED!  This was not disk one or two!!!\n");
  return 0;
}
```

`FUN_004a80d0` (verified via decompile at `0x004a80d0`) is a one-line stub that always returns `2`.
So: if called with -1 → side becomes 2 → then `param_1 != 0` is true AND `param_1 != 1` is true → returns 0 immediately, printing the disk-detection error.

**Active in YR: Conditional.** This path is reachable only if a caller passes -1.
Neither `ScenarioClass__Full_Init` nor `FUN_0067e730` (save-load restore) pass -1.
This is a legacy CD-based disk detection path that is dead in normal YR skirmish play.
Evidence: `ScenarioClass__Full_Init` passes `g_ScenarioClass_Instance[0xd2e]` which is the side derived from the house type — always 0, 1, or 2.

---

## 7. Callers

Two callers confirmed (via `get_function_callers 0x00534fa0`):

| Caller | Address | Context |
|---|---|---|
| `ScenarioClass__Full_Init` | `0x00686b20` | Called TWICE: once before `ScenarioClass__Read_INI_Basic`, once after in singleplayer mode only. Both in non-editor game path. |
| `FUN_0067e730` | `0x0067e730` | Save/load restore path — reads side from `g_ScenarioClass_Instance + 0x35a2` before calling. Passes no argument (uses global side). |

**Double-call in singleplayer:** In `ScenarioClass__Full_Init`, when `g_GameMode == 0`, `InitSideMixFiles()` is called a second time after `ScenarioClass__Read_INI_Basic` succeeds. The release loop at the start of the function handles this by freeing any previously loaded archives before re-loading them.

---

## 8. Archive Release (at start of every call)

Four globals are released at the beginning of each `InitSideMixFiles` call regardless of the new side. The release pattern for each:
```c
if (DAT_00884eXX != NULL):
  log("     Releasing %s", DAT_00884eXX[3])  // logs filename
  (*DAT_00884eXX->vtable[0])(1)              // destroy/close the CDFile
  DAT_00884eXX = NULL
```

Globals released in order: `DAT_00884e68`, `DAT_00884e74`, `DAT_00884e70`, `DAT_00884e78`.

**Important:** `DAT_00884e68` is released at function start but never written during the load phase in the decompiled output. It may be written by another system (e.g., NTRLMD.MIX loader) or is a fifth slot that was used in an earlier RA2 version. Its purpose is **unverified in this session**.

---

## 9. Post-Load Actions (inside InitSideMixFiles, after archives open)

All of the following run only if `SIDEC%02d.MIX` was successfully loaded:

1. `PaletteLoad()` at `0x0072f350` — unloads and reloads left-panel SHPs; checks
   `g_ScenarioClass_Instance + 0x34B8` to decide palette order for Yuri:
   - If side == 2: loads `DAT_00b0fbf0` variant first (DIALOGY.PAL path)
   - Else: loads `DAT_00b0fbf8` variant first
   Also calls `LeftPanel__ComputeLayoutRects()` to re-derive layout from new SHP dims.

2. `SetSidebarTextColor()` — selects 1-of-3 RGB values for sidebar text

3. Load `UIMD.INI` from the now-active MIX search path (MANDATORY — missing = return 0)

4. `RulesClass::ReadCommandBar(&DAT_00887208, is_multiplayer)` — reads command bar config

5. `FUN_006d02b0()` → `SidebarClass::LoadSHPs` + button art (Button_%02d.SHP loop)

---

## 10. Side Index in ScenarioClass

Side is stored at `g_ScenarioClass_Instance + 0x34B8` (confirmed via `PaletteLoad` decompilation
at `0x0072f350` which reads `*(int *)(g_ScenarioClass_Instance + 0x34b8)`).

In `ScenarioClass__Full_Init`, the argument to `InitSideMixFiles()` comes from
`g_ScenarioClass_Instance[0xd2e]`, which is derived from `HouseTypeClass + 0xbc` (the `.Side` field).

---

## 11. FUN_005b43f0 — Verified Stub

`FUN_005b43f0` (called between SIDEC%02dMD.MIX open and SIDEC%02d.MIX open, and again
between SIDEC%02d.MIX and SIDENC%02d.MIX) decompiles to:
```c
undefined1 FUN_005b43f0(void) { return 1; }
```
One-liner stub. No observable side effect. Active in YR: Yes (called), but functionally inert.

---

## 12. Rust Implementation Status

Current Rust code (`src/assets/asset_manager.rs` lines 90–93) lists the side archives in the static KNOWN_NESTED_MIXES array:
```
"sidec01.mix", "sidec01md.mix", "sidec02.mix", "sidec02md.mix"
```
Note: `sidenc01.mix` and `sidenc02.mix` are NOT in this array. They appear in
`src/assets/mix_diag_tests.rs` (lines 145–147) but not in the main asset manager.

`src/render/sidebar_chrome.rs` (lines 128–150) loads `sidec01.mix`, `sidec02.mix`,
`sidec02md.mix` as the three theme atlases. Current code treats Yuri as loading
`sidec02md.mix` (a separate atlas), which **diverges from gamemd behavior**: gamemd
loads `SIDEC02.MIX` + `SIDEC02MD.MIX` as a search stack for Yuri (same as Soviet),
not as a separate theme.

**Gaps vs gamemd:**
1. `SIDENC01/02.MIX` not loaded by the asset manager runtime (only in tests)
2. Yuri treated as a distinct third theme; gamemd routes Yuri→Soviet for all archive lookups
3. No release-and-reload cycle (gamemd releases all four slots on every `InitSideMixFiles` call)
4. UIMD.INI loading is not gated behind the side MIX search path in the Rust code

---

## 13. TS-Legacy Assessment

| Finding | Active in YR |
|---|---|
| Yuri→Soviet substitution (`if side==2: side=1`) | YES — unconditional |
| -1 sentinel / disk detection path | CONDITIONAL — only if caller passes -1; no caller does in normal YR play |
| `FUN_005b43f0` stub calls | YES — called but inert |
| Archive release loop at start | YES — called on every invocation |
| Double-call in singleplayer mode | YES — `g_GameMode == 0` path fires in campaign |
| `RulesClass::ReadCommandBar` with `is_multiplayer=1` | YES — fires for `g_GameMode != 0 and != 5` |

---

## 7. Open Questions — Final State

- `[RESOLVED]` Q1 — What is the exact Yuri→Soviet substitution? → `if (param_1 == 2) param_1 = 1;` before side+1 increment (evidence: decompile `0x00534fa0`)
- `[RESOLVED]` Q2 — What format strings are used? → `SIDENC%02d.MIX`, `SIDEC%02d.MIX`, `SIDEC%02dMD.MIX` at `0x00827dd4`, `0x00827de4`, `0x00827e0c` (evidence: `read_memory 0x00827dd4` length 80)
- `[RESOLVED]` Q3 — What is the load order? → SIDEC%02dMD first (optional), SIDEC%02d second (mandatory), SIDENC%02d third (optional, gated on SIDEC%02d success)
- `[RESOLVED]` Q4 — Who calls InitSideMixFiles? → `ScenarioClass__Full_Init` (`0x00686b20`) and save/load `FUN_0067e730` (`0x0067e730`) (evidence: `get_function_callers 0x00534fa0`)
- `[RESOLVED]` Q5 — Is it called with a side argument directly? → Yes, `g_ScenarioClass_Instance[0xd2e]` (side from house) is passed; side is at `ScenarioClass+0x34B8` (evidence: decompile `0x00686b20` and `0x0072f350`)
- `[RESOLVED]` Q6 — What does `FUN_004a80d0` return? → Constant 2, always (evidence: decompile `0x004a80d0`)
- `[RESOLVED]` Q7 — What does `FUN_005b43f0` do? → Returns 1 unconditionally, no side effects (evidence: decompile `0x005b43f0`)
- `[RESOLVED]` Q8 — Is SIDENC loaded before or after SIDEC? → After SIDEC, and gated on SIDEC being non-null (evidence: decompile `0x00534fa0`)
- `[RESOLVED]` Q9 — Is the double-call in singleplayer real? → Yes, `g_GameMode == 0` path in `ScenarioClass__Full_Init` calls it twice; the release loop at the top handles it (evidence: decompile `0x00686b20`)
- `[RESOLVED]` Q10 — What happens inside PaletteLoad for Yuri vs Allied? → Yuri path swaps order of two palette loads; both call `LeftPanel__ComputeLayoutRects` (evidence: decompile `0x0072f350`)
- `[DEFERRED]` Q11 — What does `DAT_00884e68` store? (category: `requires-different-system-context`; reason: released at function start but never assigned inside `InitSideMixFiles`; likely written by NTRLMD.MIX or global init system; next-step: search xrefs to `0x00884e68`)
- `[DEFERRED]` Q12 — Full contents of `FUN_0067e730` save-load restore path (category: `out-of-scope`; reason: save/load restore is a separate system; the call to `InitSideMixFiles` was confirmed; full save-load logic is out of scope for this investigation)
- `[DEFERRED]` Q13 — What filenames does `SidebarClass::LoadSHPs` load inside `FUN_006d02b0`? (category: `out-of-scope`; reason: downstream of InitSideMixFiles; covered partially by [SIDEBAR_CONSTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_CONSTRUCTION_GHIDRA_REPORT.md) §12)

---

## Sources

- Decompiled: `0x00534fa0` (InitSideMixFiles, full body)
- Decompiled: `0x00686b20` (ScenarioClass__Full_Init, caller)
- Decompiled: `0x0067e730` (save/load restore, caller — output truncated but InitSideMixFiles call confirmed)
- Decompiled: `0x004a80d0` (FUN_004a80d0 — constant-2 stub)
- Decompiled: `0x005b43f0` (FUN_005b43f0 — returns-1 stub)
- Decompiled: `0x0072f350` (PaletteLoad — confirms `ScenarioClass+0x34B8` side read)
- Decompiled: `0x006d02b0` (post-init sidebar SHP loader)
- `read_memory 0x00827dd4` (80 bytes) — format strings verified
- `read_memory 0x00827e1c` (40 bytes) — debug string verified
- `search_strings "Preparing Mixfiles"` — confirms string at `0x00827e1c`
- `search_strings "SIDEC"` — confirms `0x00827de4` and `0x00827e0c`
- `get_function_callers 0x00534fa0` — two callers confirmed
- Cross-reference: [SIDEBAR_CONSTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SIDEBAR_CONSTRUCTION_GHIDRA_REPORT.md) §6 and §13
- Cross-reference: `src/assets/asset_manager.rs` lines 90–93, `src/render/sidebar_chrome.rs` lines 128–150
