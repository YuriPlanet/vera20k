# Random Map Setup Dialog (0x105) — Saved-Seed Slots (Load/Save/Delete) — Ghidra Research Report

**Address(es):** `0x005587F0` (Load trampoline), `0x00558810` (Save trampoline), `0x00558840` (Delete trampoline), `0x00559C20` (availability predicate), `0x00558DD0` (`LoadOptionsClass::RunModalLoop`), `0x00622650` (generic dialog-creation helper), `0x00595680` (`MapSeedClass::Constructor`), vtable `0x007ED8E4` (`vtable__MapSeedClass`)
**Investigation Mode:** narrow-scope (`/re-swarm` slot 1 of batch)
**Claimed Scope:** the four functions named in the task (Load/Save/Delete trampolines + availability predicate), their dispatch into `LoadOptionsClass::RunModalLoop`, the dialog resources it creates, and the `MapSeedClass` vtable slots those trampolines reach.
**Non-Scope:** dialog 0x105's own geometry/controls/defaults/clamp table (already documented — see `SKIRMISH_RANDOM_MAP_SETUP_DIALOG_CONTROLS_OPTIONS_GHIDRA_REPORT.md`), the `.SED` byte-level key/value layout (already fully documented — see [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md), cited throughout instead of re-derived), the terrain generator, the preview rasterizer.
**Confidence:** High for dispatch/`this`-pointer wiring, dialog resource IDs, extension/exclusion filenames, and the Delete/availability predicate bodies (all directly decompiled or disassembled with a defined function boundary). Medium/Unverified for the exact instruction-level content of the concrete Load (`0x00597A30`) and Save (`0x00597760`) vtable bodies — Ghidra has no `Function` object at those addresses this session (see Remaining Uncertainty); the byte-level `.SED` field layout is instead taken from the already-verified sibling report.
**Active in YR:** Yes. This is the standard offline Skirmish "Create Random Map" dialog (`RT_DIALOG 0x105`) Load/Save/Delete buttons; no TS-only gate found on any function in this slice.

## Working Notes Gate

- **Target question:** What is the saved-seed slot mechanism (file format/location, slot count/keys, dialog UI, `MapSeedClass` write-back on Load, availability-predicate semantics, Save's `&DAT_00ABE050` argument identity) behind `RandomMapSetupDialog::Proc` cases `0x6C2`/`0x6C3`/`0x6C4` and `WM_INITDIALOG`?
- **Non-goals:** dialog 0x105 geometry/defaults/Randomize order/OK-Cancel gates; the terrain generator; the preview rasterizer; anything not reached from the four named functions or their immediate callees.
- **Evidence needed to mark COMPLETE:** decompile of all four target functions; caller/xref confirmation of the `this`-pointer and call sites inside `RandomMapSetupDialog::Proc`; identification of the dialog resource(s) `LoadOptionsClass::RunModalLoop` creates; identification of the `MapSeedClass` vtable slots the trampolines dispatch through; the extension/exclusion strings the availability predicate and its virtual callee test.
- **Stop conditions:** stop once the four functions + their direct dispatch chain (`RunModalLoop`, the dialog-creation helper, the vtable) are cited: reached that point; the concrete vtable body internals for Load/Save are UNCHECKED this session because Ghidra has no function boundary there (see Remaining Uncertainty) and creating one is forbidden by the swarm's hard constraints.

## 1. Overview

The four target functions are **not** a bespoke "seed slot" UI. They are thin mode-setters on a shared, engine-wide **`LoadOptionsClass`** (base class name from `MapSeedClass::Constructor`'s call to `LoadOptionsClass::Constructor`, verified via `decompile_function 0x00595680`) — the exact same base class that backs the ordinary main-menu **Load Game / Save Game / Delete Game** dialogs (`FUN_005587F0` is also called from `Main_Game @ 0x0052D9A0`, verified via `get_function_callers 0x005587F0`). `MapSeedClass` (the RMG option record at `DAT_00ABDFD8`, already known from prior work) derives from `LoadOptionsClass` and overrides its virtual Load/Save/Delete/"file matches" slots to operate on `.SED` seed files instead of full save-game state.

Each of the three WndProc cases (`0x6C2`/`0x6C3`/`0x6C4`) loads `ECX = 0xABDFD8` immediately before calling its trampoline (verified via `get_assembly_context` at call sites `0x0059694A`/`0x005968DC`/`0x005969CA` — each shows `MOV ECX,0xabdfd8` directly preceding the `CALL`). So the trampolines' `this` **is** the live `MapSeedClass` record itself, not a separate dialog-controller object; the fields the trampolines mutate (`+0x4` mode, `+0xC` description-override pointer) are `LoadOptionsClass` base-class fields that live *inside* the same 0x178-byte `MapSeedClass` record, below the RMG-specific fields that start at `+0x38`.

## 2. Class Layout / Key Offsets (base-class fields touched by this slice)

| Field (relative to `DAT_00ABDFD8`) | Meaning | Evidence | Active in YR |
|---|---|---|---|
| `+0x0` | vtable pointer, set to `vtable__MapSeedClass` (`0x007ED8E4`) at construction | `decompile_function 0x00595680` (`*param_1 = &vtable__MapSeedClass;`) | Yes |
| `+0x4` | Load/Save/Delete mode: `1`=Load, `2`=Save, `3`=Delete | `decompile_function 0x005587F0/0x00558810/0x00558840` | Yes |
| `+0x8` | pointer to the 3-char extension string `"SED"` (`PTR_LAB_0082BA60`) | `decompile_function 0x00595680`; `read_memory 0x0082ba60` -> bytes `53 45 44 00` = `"SED"` | Yes |
| `+0xC` | description-override pointer; defaults to **self** (`&this+0x78`, i.e. the object's own embedded Description field) at construction | `decompile_function 0x00595680` (`param_1[3] = puVar1;` where `puVar1 = param_1+0x1e` = `+0x78`) | Yes |
| `+0x10` | (unrelated to base-class dispatch here — this is `param_1[4]` in `LoadOptionsClass::RunModalLoop`, a required-free-disk-space threshold, not examined further; out of scope) | `decompile_function 0x00558DD0` | Conditional |
| `+0x78` | `Description` (already-known RMG field) — `DAT_00ABDFD8+0x78 == DAT_00ABE050` exactly | arithmetic + `decompile_function 0x00595680` | Yes |

Vtable `vtable__MapSeedClass @ 0x007ED8E4` slots relevant to this slice (read via `read_memory 0x007ED8E4` length 40, little-endian words):

| Slot (vtable+offset) | Target address | Role (from dispatch context in `RunModalLoop`) |
|---|---|---|
| `+0x00` | `0x005AC270` | not exercised in this slice (likely destructor) |
| `+0x04` | `0x00597A30` | `Load(filename) -> bool` |
| `+0x08` | `0x00597760` | `Save(filename, descriptionPtr) -> bool` |
| `+0x0C` | `0x00597D50` | `Delete(filename) -> bool` |
| `+0x10` | `0x00597D60` | `MatchesFile(outDescBuf, WIN32_FIND_DATA*) -> bool` (used by the availability predicate) |
| `+0x14`/`+0x18`/`+0x1C` | `0x00597F80`/`0x00597FA0`/`0x00597FC0` | per-mode dialog caption/title getters |
| `+0x20` | `0x00597FE0` | post-save status-message getter |

Evidence for the table: `read_memory 0x007ED8E4` (raw vtable bytes) cross-checked against `get_assembly_context` at the call sites inside `LoadOptionsClass::RunModalLoop` (`(**(code**)(*param_1+4))(...)`, `+8`, `+0xc`, `+0x10`, `+0x14`, `+0x18`, `+0x1c`, `+0x20`, all present verbatim in `decompile_function 0x00558DD0`).

## 3. Core Logic

### 3.1 The four target functions (fully decompiled, defined `Function` objects)

```
FUN_005587F0 (Load):   this[+4]=1 (mode=Load); this[+0xC]=0 (no description override); call RunModalLoop()
FUN_00558810 (Save):   this[+4]=2 (mode=Save); this[+0xC] = arg if arg!=0 else (g_ScenarioClass_Instance+0x1360); call RunModalLoop()
FUN_00558840 (Delete): this[+4]=3 (mode=Delete); this[+0xC]=0; call RunModalLoop()
FUN_00559C20 (predicate): enumerate "*.SED" via FindFirstFileA/FindNextFileA, testing each candidate
```
Evidence: `decompile_function` on all four addresses (verbatim bodies obtained). Active in YR: Yes — these are the direct callees of `RandomMapSetupDialog::Proc` cases `0x6C2`/`0x6C3`/`0x6C4` and `WM_INITDIALOG`, verified via `get_function_callers` on all four addresses (each lists `RandomMapSetupDialog__Proc @ 00596300`).

**`this` binding (answers "what object do these operate on"):** `get_assembly_context` on the exact call sites inside `RandomMapSetupDialog::Proc` (`0x0059694F`, `0x005968E1`, `0x005969CF`, plus the three `FUN_00559C20` call sites `0x00596BEB`/`0x005968EB`/`0x005969D9`) shows `MOV ECX,0xabdfd8` immediately before every one of these `CALL`s. **`this == DAT_00ABDFD8`, the live global `MapSeedClass` record, in every RMG-dialog call site.** Active in YR: Yes.

### 3.2 Q1 — File format and location

- **Extension:** `"SED"` (3 ASCII bytes, no leading dot; the wildcard format string `"*.%3s"` at `0x0082B9F0`/verified via `read_memory 0x00829f7c` supplies the dot) is stored at `MapSeedClass+0x8`, set at construction from `PTR_LAB_0082ba60` -> `read_memory 0x0082ba60` = bytes `53 45 44 00` = `"SED"`. Active in YR: Yes.
- **Search location:** `FUN_00559C20` builds the `FindFirstFileA` pattern with `FUN_007C8EF4(local_33c, "*.%3s", extensionPtr)`. `decompile_function 0x007c8ef4` shows this is a plain `_vsnprintf`-style formatter (calls the CRT internals `FUN_007ce2a5`/`FUN_007ce18d`) with **no directory component ever concatenated**. The resulting pattern is the bare wildcard `"*.SED"`, so `FindFirstFileA`/`FindNextFileA` search the **process's current working directory** — there is no dedicated "Saved Games"-style folder for this feature. Active in YR: Yes (this is the same enumeration the availability predicate and, by construction, the Save-dialog's file list use).
- **Byte-level `.SED` layout** (section name, exact key order, integer-vs-hex-UTF16 encoding, normalizer bounds, per-key defaults): **already fully verified** in [docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) (Sections 2–3 there). That report used the same writer/reader addresses this slice reaches (`0x00597760` write body, `0x00597A30` read body, reached via vtable `+8`/`+4`) and is not re-derived here — see its Section 2 table for the complete `[RandomMap]` key list and Section 3.5 for the `Description` hex-UTF16-CSV encoding.

### 3.3 Q2 — Slot count and naming

There is **no fixed slot count and no numeric/registry key**. Saving is a normal "type a filename" flow: `LoadOptionsClass::RunModalLoop`'s Save branch (`decompile_function 0x00558DD0`, `iVar9==2` branch) retrieves up to 0x50 (80) characters of player-typed text from edit control `0x526` via a custom `SendMessageA(..., 0x4B3, 0x50, buf)` call, and that text becomes the base filename before the `.SED` extension is appended by the writer. This exactly mirrors the ordinary Save-Game dialog's filename entry (same `LoadOptionsClass::RunModalLoop`, mode `2`), which is expected since it is the same code. Active in YR: Yes.

**Two filenames are structurally reserved and excluded from the browsable list**, discovered via the `MatchesFile` vtable override (`0x00597D60`, dispatched from the availability predicate at vtable `+0x10`):
- `"RandMap.Sed"` — `read_memory 0x0082bc30` = `52 61 6e 64 4d 61 70 2e 53 65 64 00` = `"RandMap.Sed"`. This is the **same string address** (`0x0082BC30`) documented in [SKIRMISH_RANDMAP_SED_RANDOM_MAP_BEHAVIOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_RANDOM_MAP_BEHAVIOR_GHIDRA_REPORT.md) and [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) as the **active/transient** random-map file the Choose-Map "Create Random Map" accept path writes and the `.SED` scenario-launch path reads — i.e. it is excluded from the saved-seed browser precisely because it is the *working* file for "the map currently queued for this match", not a player-named saved slot.
- `"lastmap.sed"` — `read_memory 0x0082bc24` = `6c 61 73 74 6d 61 70 2e 73 65 64 00` = `"lastmap.sed"`. **Not documented in either prior `.SED` report** — this is a second reserved filename, presumably a separate auto-persisted "last used RMG settings" cache, excluded from the browsable slot list the same way.

Evidence: `get_assembly_context 0x00597D60` context showed `LEA EDI,[EAX+0x2C]` (== `WIN32_FIND_DATA.cFileName`, offset `0x2C` in that struct) then two back-to-back `PUSH <string>; PUSH EDI; CALL 0x007c8d20` comparisons against `0x0082BC30` and `0x0082BC24`, each gated by `JZ <exit>` on a zero (non-match) result — i.e. a candidate file is rejected from the list if its name equals either reserved string. `decompile_function 0x007c8d20` confirms this helper is a case-insensitive string-equality test (classic uppercase-normalize-then-compare idiom, functionally `_stricmp`). Active in YR: Yes.

### 3.4 Q3 — Dialog UI: real Windows common dialog or RA2 custom dialog?

**RA2 custom dialog**, not a Windows common dialog (no `GetOpenFileNameA`/`GetSaveFileNameA` anywhere in this call chain). `LoadOptionsClass::RunModalLoop` (`0x00558DD0`) branches on mode and, per branch, loads a specific **RT_DIALOG resource ID** into `ECX` and a specific `DLGPROC` into `EDX` immediately before calling the generic dialog-creation helper `FUN_00622650`:

| Mode | Resource ID | DLGPROC | Evidence |
|---|---|---|---|
| Load (`1`) | `0xB7` | `0x00558A30` | `get_assembly_context 0x00558F39` -> `MOV ECX,0xb7 / MOV EDX,0x558a30 / CALL 0x00622650` |
| Save (`2`) | `0x2B4` | `0x00558B90` | `get_assembly_context 0x00558EC1` -> `MOV ECX,0x2b4 / MOV EDX,0x558b90 / CALL 0x00622650` |
| Delete (`3`) | `0x2B5` | `0x00558CB0` | `get_assembly_context 0x00558E03` -> `MOV ECX,0x2b5 / MOV EDX,0x558cb0 / CALL 0x00622650` |

`FUN_00622650` is confirmed to be a generic `CreateDialogIndirectParamA(hInstance, lpTemplate, g_hWnd, dlgProc, lParam)` wrapper (`decompile_function 0x00622650`) — it has **38 distinct callers across the engine** (`get_xrefs_to 0x00622650`; e.g. `OptionsClass__ShowLauncherDialog`, `OptionsClass__ShowInGameDialog`, `WebBrowser__Constructor`, `SimpleWonlineDialogControl__Constructor`), confirming it is the standard "show a native RA2 dialog resource" helper, not a per-feature file-picker. Populated via custom `SendMessageA` control messages (`0x182`/`0x186`/`0x188`/`0x199`/`0x18B`, a listbox-style message range) rather than a native common-dialog callback. Active in YR: Yes.

### 3.5 Q4 — What Load writes back, and does it re-clamp

**Dispatch is fully verified; the concrete field-by-field content of the Load body is UNCHECKED this session** (Ghidra has no `Function` boundary at `0x00597A30` — see Remaining Uncertainty). What is verified:

- `RandomMapSetupDialog::Proc` case `0x6C2` calls `FUN_005587F0()` with `this=DAT_00ABDFD8` (mode=Load, no description override), which shows the Load dialog (resource `0xB7`) via `RunModalLoop`. On file selection, `RunModalLoop`'s mode-1 branch dispatches `(**(code**)(*param_1+4))(filename)` — i.e. vtable `+0x4` = `0x00597A30` — **with `this` still `DAT_00ABDFD8`**, so any field writes land directly in the live global RMG option record. `decompile_function 0x00596300` (full `RandomMapSetupDialog::Proc` body, case `0x6C2`).
- After `FUN_005587F0` returns non-zero (success), the WndProc's `0x6C2` handler calls `RandomMapSetupDialog__SyncControlsFromOptions(param_1)` (`decompile_function 0x00596300`). That function's own body calls `MapSeedClass::ClampFields` **at its top**, before refilling controls — verified via `get_xrefs_to 0x005975E0` (lists caller `0x00596E59 in RandomMapSetupDialog__SyncControlsFromOptions`) and the already-existing plate comment on `0x00596E50` ("Clamps first (MapSeedClass__ClampFields 0x005975E0), then per combo..."). **So: Load does re-clamp, but only as a side effect of the WndProc's post-Load `SyncControlsFromOptions` call — not inside the Load vtable body itself** (no caller edge from `0x00597A30` to `0x005975E0` was found in `get_function_callers 0x005975E0`, whose only callers are `FUN_00596C70`, `FUN_00597380`, `RandomMapSetupDialog__Proc`, and `RandomMapSetupDialog__SyncControlsFromOptions`).
- The exact field list/order the Load body (`0x00597A30`) itself reads and writes is documented at the byte level in [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) Section 3.8: reader consumes the same `[RandomMap]` keys as the writer, using **current object field values as per-key defaults** for any missing key (so a partially-written or hand-edited `.SED` leaves untouched fields at whatever the in-memory record already held), and that report found **no reader-side call to the normalizer** inside the reader body itself — consistent with what this slice found (the clamp comes from `SyncControlsFromOptions`, called by our WndProc case, not from the reader).

### 3.6 Q5 — What the availability predicate actually tests

`FUN_00559C20` (fully decompiled, defined function) tests **file presence**, not a count or registry key:

1. Builds pattern `"*.SED"` (no directory) and calls `FindFirstFileA`.
2. For each match, requires `(dwFileAttributes & 0x116) == 0` — i.e. rejects `HIDDEN`(0x2)|`SYSTEM`(0x4)|`DIRECTORY`(0x10)|`TEMPORARY`(0x100) entries.
3. Requires the filename is not (case-insensitively) `"SAVEGAME.NET"` via `FUN_007C8D20` (`read_memory 0x00820dac` = `"SAVEGAME.NET"`) — this is the engine-wide multiplayer-autosave exclusion shared by every `LoadOptionsClass`-derived dialog, not RMG-specific.
4. Calls the virtual `MatchesFile` slot (vtable `+0x10` = `0x00597D60` for `MapSeedClass`), which additionally excludes `"RandMap.Sed"`/`"lastmap.sed"` (Section 3.3) and populates a description buffer; the predicate also requires an internal error/flag byte (`local_147`) to be zero.
5. Returns `1` (available) on the **first** file that passes all checks; `0` if `FindFirstFileA` fails or no candidate passes.

Evidence: `decompile_function 0x00559C20` (verbatim body); `decompile_function 0x00597D60` context via `get_assembly_context` for the exclusion compares; `read_memory` on both exclusion strings and `"SAVEGAME.NET"`. Called from `WM_INITDIALOG` (`0x00596BEB`) to gate initial Load/Delete button enable state, and again after Save (`0x005968EB`) and after Delete (`0x005969D9`) to refresh that gate — all confirmed via `get_xrefs_to 0x00559C20`. Active in YR: Yes.

### 3.7 Q6 — What Save writes; identity of `&DAT_00ABE050`

`&DAT_00ABE050` is **not a separate global** — it is `DAT_00ABDFD8 + 0x78` exactly, i.e. the address of `MapSeedClass`'s own **`Description`** field (the same offset already known from prior RMG-dialog research). Two independent confirmations:

1. Arithmetic: `0xABDFD8 + 0x78 = 0xABE050`.
2. `MapSeedClass::Constructor` (`decompile_function 0x00595680`) initializes the base-class description-override pointer (`this+0xC`) to **exactly this same address** by default: `puVar1 = param_1 + 0x1e` (word-indexed, `= this+0x78`); `param_1[3] = puVar1;` (`param_1[3]` is `this+0xC`).

Since `FUN_005587F0` (Load) and `FUN_00558840` (Delete) both **zero** `this+0xC` (`decompile_function` on each), any Load or Delete click clears the description-override pointer. The WndProc's Save case (`0x6C3`) therefore **must** re-pass `&DAT_00ABE050` explicitly to `FUN_00558810` to restore it to the object's own Description field — otherwise `FUN_00558810`'s `param_2==0` fallback (`param_2 = g_ScenarioClass_Instance + 0x1360`) would point the Save dialog at the *regular game's* scenario description instead of the RMG record's own. `LoadOptionsClass::RunModalLoop`'s Save branch (`decompile_function 0x00558DD0`) then uses this pointer (`param_1[3]`) to **pre-fill the Save dialog's filename/description edit control (`0x526`)** via a custom `SendMessageA(..., 0x4B2, 0, LVar3)` set-text call — so the practical effect is: opening Save after a Load/Delete still shows the *current* seed's description text as the suggested save name, not blank or the wrong scenario's text. Active in YR: Yes.

The actual byte content the Save vtable body (`0x00597760`) writes to disk (section, key order, encodings) is already fully documented in [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) Sections 3.3–3.5 and is not re-derived here.

### 3.8 Delete's actual effect

The Delete vtable body (`0x00597D50`, has a defined instruction stream even though no unique `Function` name was assigned) is trivial: `MOV EAX,[ESP+4]` (filename arg) `; PUSH EAX ; CALL 0x00559EB0 ; RET 0x4`. `decompile_function 0x00559eb0` shows that helper is literally `DeleteFileA(param_1) == TRUE`. **Delete performs no `MapSeedClass` field mutation** — it only removes the chosen `.SED` file from disk. Active in YR: Yes.

## 4. INI Keys

Not applicable to this slice — the persisted `.SED` key/value layout is [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md)'s subject, not re-derived here. No `rules(md).ini`/`art(md).ini` keys are touched by any of the four target functions.

## 5. Integration Points

| Integration | Behavior | Evidence | Active in YR |
|---|---|---|---|
| `RandomMapSetupDialog::Proc` case `0x6C2` | `this=DAT_00ABDFD8`; calls `FUN_005587F0` -> Load dialog `0xB7`; on success, `SyncControlsFromOptions` re-clamps + resyncs | `decompile_function 0x00596300` | Yes |
| case `0x6C3` | `this=DAT_00ABDFD8`; calls `FUN_00558810(&DAT_00ABE050)` -> Save dialog `0x2B4`; then re-checks availability to toggle Load/Delete enable | `decompile_function 0x00596300` | Yes |
| case `0x6C4` | `this=DAT_00ABDFD8`; calls `FUN_00558840` -> Delete dialog `0x2B5`; then re-checks availability | `decompile_function 0x00596300` | Yes |
| `WM_INITDIALOG` (`0x497`) | calls `FUN_00559C20` once to gate initial Load/Delete enable state; OK/Save start disabled | `decompile_function 0x00596300` | Yes |
| Shared base class | `MapSeedClass` derives from `LoadOptionsClass`, the same base used by the main-menu Load/Save/Delete Game dialogs (`Main_Game` is also a caller of `FUN_005587F0`) | `get_function_callers 0x005587F0`; `decompile_function 0x00595680` | Yes |
| `.SED` launch path (separate flow, cross-referenced not re-derived) | `ScenarioClass__Read_Scenario` reaches the same reader vtable slot (`+0x4` = `0x00597A30`) via `FUN_00597A10` when starting a `RandMap.Sed`-selected match | [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) §3.8 (xref `0x00684975`, not re-verified this session) | Conditional |

## 6. Current Rust Implementation Status

Per the parent task's brief (not re-scanned in depth this session, since Rust-file inspection was out of scope for this slice — only the two file paths named in the task brief are noted):

| Area | Current Rust status | Evidence |
|---|---|---|
| `.SED` codec | `RmgOptions` + hex-UTF16 `.SED` INI codec exists, targeting a single file `RandMap.Sed` | `src/map/rmg/options.rs` (per task brief; not re-read this session) |
| Saved-seed slots (Load/Save/Delete) | Dialog-state match arms are empty stubs; `Control::Load0x6c2`/`Save0x6c3`/`Delete0x6c4` are no-ops | `src/ui/skirmish_shell/state/random_map_setup.rs`, `src/app.rs` (per task brief; not re-read this session) |

This session did not independently re-verify these Rust file states — see the parent swarm brief for the authoritative current-Rust snapshot.

## 7. Coverage Ledger

| Area / function | Status | Evidence | What remains |
|---|---|---|---|
| Four target functions (dispatch + `this`) | verified | decompile + `get_assembly_context` on all call sites | none |
| Dialog resource IDs / DLGPROCs per mode | verified | `get_assembly_context` at `0x00558E03`/`0x00558EC1`/`0x00558F39` | none |
| Generic-dialog-helper identity (RA2 vs Windows common dialog) | verified | `decompile_function 0x00622650`; `get_xrefs_to 0x00622650` (38 callers) | none |
| Extension string `"SED"` | verified | `read_memory 0x0082ba60` | none |
| No-directory-prefix search (CWD) | verified | `decompile_function 0x007c8ef4` | runtime CWD value itself not observed (static analysis only) |
| Exclusion filenames `RandMap.Sed`/`lastmap.sed` | verified | `read_memory 0x0082bc30`/`0x0082bc24`; `get_assembly_context 0x00597D60` | none |
| Availability predicate full logic | verified | `decompile_function 0x00559C20` | none |
| Delete effect (`DeleteFileA` only) | verified | `decompile_function 0x00559eb0`; assembly at `0x00597D50` | none |
| Description-pointer identity (`&DAT_00ABE050 == this+0x78`) | verified | arithmetic + `decompile_function 0x00595680` + `decompile_function 0x00558DD0` (Save branch pre-fill use) | none |
| Load/Save clamp timing (`SyncControlsFromOptions`, not the vtable body) | verified | `get_xrefs_to 0x005975E0`; `decompile_function 0x00596300` | none |
| Concrete Load/Save vtable body field-by-field content | **not independently verified this session** | N/A — no `Function` boundary at `0x00597A30`/`0x00597760` | already answered by [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md); cited, not re-derived |

## 8. Open Questions — Final State

- `[RESOLVED] Q1 file format/location -> extension "SED", search pattern has no directory component (CWD-relative); byte-level key layout already documented elsewhere (cited).`
- `[RESOLVED] Q2 slot count/keys -> unbounded, player-typed filename via edit control 0x526 (max 80 chars); two filenames ("RandMap.Sed", "lastmap.sed") are structurally reserved/excluded from the list.`
- `[RESOLVED] Q3 dialog UI -> genuine RA2 RT_DIALOG resources (0xB7 Load / 0x2B4 Save / 0x2B5 Delete), created via the engine-wide CreateDialogIndirectParamA wrapper; not a Windows common dialog.`
- `[RESOLVED] Q4 Load write-back -> this=DAT_00ABDFD8 directly; clamp happens via SyncControlsFromOptions after a successful Load, not inside the Load vtable body; exact field content deferred to the existing writer/reader layout report.`
- `[RESOLVED] Q5 availability predicate -> pure file-presence test over "*.SED" in CWD, with attribute mask 0x116 exclusion, "SAVEGAME.NET" exclusion (generic), and MapSeedClass-specific "RandMap.Sed"/"lastmap.sed" exclusion via the virtual MatchesFile slot.`
- `[RESOLVED] Q6 &DAT_00ABE050 identity -> MapSeedClass's own Description field (this+0x78), matching the constructor's self-referential default; re-passed explicitly because Load/Delete zero the override pointer.`
- `[DEFERRED] What are the exact instructions inside the concrete Load (0x00597A30) / Save (0x00597760) vtable bodies? -> No Ghidra Function boundary this session; the byte-level content is separately and already verified in SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md, so this is a tooling gap, not an open behavioral question.`

## 9. Implementation Handoff

| Verified behavior | Rust delta | Affected surface | Acceptance scenario | Proposed test name | Risk |
|---|---|---|---|---|---|
| Saved-seed slots are arbitrary player-named `.SED` files enumerated from the process's current working directory via `"*.SED"`, excluding `SAVEGAME.NET`, `RandMap.Sed`, and `lastmap.sed`. No fixed slot count. | `src/ui/skirmish_shell/state/random_map_setup.rs` Load/Save/Delete stubs need a directory scan (CWD-relative, matching game install dir) that lists `.SED` files minus the three reserved names, and a Save flow that accepts a player-typed filename (not a numbered slot). | `state/random_map_setup.rs`, `src/app.rs` (`Control::Load0x6c2`/`Save0x6c3`/`Delete0x6c4`) | `test_saved_seed_scan_excludes_randmap_and_lastmap_sed`: a directory containing `RandMap.Sed`, `lastmap.sed`, `SAVEGAME.NET`, and `MySeed.SED` yields exactly one browsable entry (`MySeed.SED`). | Medium — needs a real filesystem fixture, not just a unit stub. |
| Load re-clamps as a side effect of the post-load control resync, not inside the load/parse step itself; a loaded `.SED` with out-of-range values must still be clamped (except Theater, per the existing `ClampFields` gap already documented) before the dialog displays it. | Ensure the Rust Load handler calls the equivalent of `ClampFields`+control-resync after applying loaded fields, matching native order (clamp happens in the resync call, not the reader). | `random_map_setup.rs` Load handler; existing `RmgOptions` clamp logic | `test_loaded_seed_is_clamped_after_load_matching_sync_order`: a hand-crafted `.SED` with `NumPlayers=99` loads then displays clamped to `8`, same as native `ClampFields` bound. | Low — clamp bounds are already ported per parent context; this only adds the "clamp after Load, in the resync step" ordering. |
| Save must pre-fill the save-name/description field from the *current* in-memory seed's Description, even immediately after a Load or Delete (native re-establishes this pointer explicitly because Load/Delete null it). | Rust's Save flow should always source the suggested filename/description from the live `RmgOptions.description`, never from a stale/blank value left over from a prior Load/Delete click. | `random_map_setup.rs` Save handler | `test_save_dialog_prefill_uses_current_description_after_load`: Load a seed, then immediately open Save — the suggested description/filename reflects the just-loaded seed's Description, not blank or the previous seed's. | Low — UI pre-fill only, no format risk. |

## Negative Facts / Do Not Do

- Do not build a fixed-count "slot" model (e.g. Slot 1–N). Native has no numeric slots — it is a plain filename-per-save list, identical in shape to the ordinary Save-Game dialog. Evidence: `decompile_function 0x00558DD0` (mode-2 branch reads a free-text edit control, not an indexed list selection, for the *new* save name).
- Do not treat `RandMap.Sed` or `lastmap.sed` as player-visible saved slots. Both are excluded by the `MatchesFile` vtable override. Evidence: `read_memory 0x0082bc30`/`0x0082bc24`; `get_assembly_context 0x00597D60`.
- Do not implement this as a Windows common file-open/save dialog (`GetOpenFileNameA`/`GetSaveFileNameA`). Native uses RA2's own `RT_DIALOG` resources (`0xB7`/`0x2B4`/`0x2B5`) via the engine-wide `CreateDialogIndirectParamA` wrapper. Evidence: `decompile_function 0x00622650`; `get_xrefs_to 0x00622650`.
- Do not assume Load calls the clamp function directly. The clamp is a side effect of `RandomMapSetupDialog::SyncControlsFromOptions`, called by the WndProc *after* a successful Load — not by the Load vtable body itself. Evidence: `get_function_callers 0x005975E0` (does not list the Load body among callers).
- Do not assume Delete mutates any in-memory `MapSeedClass` state. It only calls `DeleteFileA` on the chosen file. Evidence: `decompile_function 0x00559eb0`; assembly at `0x00597D50`.
- Do not search a dedicated "Saved Games"-style directory for `.SED` files. The native `FindFirstFileA` pattern has no directory component, so the search (and by construction, presumably read/write) is relative to the process's current working directory. Evidence: `decompile_function 0x007c8ef4`.

## Remaining Uncertainty

- The concrete Load (`0x00597A30`) and Save (`0x00597760`) vtable-body instruction streams have **no Ghidra `Function` object** this session — `decompile_function`, `get_function_by_address`, and `disassemble_function` all report "no function found," and `disassemble_bytes` reports the bytes are already disassembled as orphan/unbounded code. Per the swarm's hard constraint, creating that function boundary was not attempted. This is a **tooling gap, not an unresolved behavioral question** — [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) already independently derived the full field-by-field key order and encoding for these same two bodies (apparently via manual disassembly walk in a prior session) and is cited throughout this report instead of re-deriving it.
- The runtime value of the process's current working directory at the moment the availability predicate/Load/Save/Delete run was not observed (static analysis only) — the "searches CWD" conclusion is drawn from the format string having no directory component, not from an observed live path.
- `param_1[4]` (`MapSeedClass+0x10`, a free-disk-space threshold gate inside `LoadOptionsClass::RunModalLoop`'s Save branch) was noted but not investigated — out of scope for this slice's questions.
- Slot `+0x0`'s vtable target (`0x005AC270`, presumably a destructor) and slots `+0x14`/`+0x18`/`+0x1C`/`+0x20` (per-mode caption/message getters) were identified by address only, not decompiled — out of scope (they don't affect file format, write-back, or availability semantics).

## Stale Docs / Follow-up Docs

None found. [SKIRMISH_RANDMAP_SED_RANDOM_MAP_BEHAVIOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_RANDOM_MAP_BEHAVIOR_GHIDRA_REPORT.md) and [SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md) are both consistent with everything found in this slice (same string address `0x0082BC30` for `"RandMap.Sed"`, same vtable-dispatch shape for the writer/reader wrappers `FUN_00597730`/`FUN_00597A10`). This report adds the dialog-level mechanics (resource IDs, availability predicate, `lastmap.sed` exclusion, description-pointer identity) that those two reports explicitly scoped out; no corrections to their content are needed.

## Sources

- Ghidra read-only decompile: `0x005587F0`, `0x00558810`, `0x00558840`, `0x00559C20`, `0x00558DD0`, `0x00622650`, `0x00595680`, `0x00596300`, `0x005975E0`, `0x007c8ef4`, `0x007c8d20`, `0x00559eb0`, `0x00597730`.
- Ghidra read-only assembly/context: `get_assembly_context` at `0x0059694A/0x0059694F`, `0x005968DC/0x005968E1`, `0x005969CA/0x005969CF`, `0x00596BEB/0x005968EB/0x005969D9`, `0x00558E03`, `0x00558EC1`, `0x00558F39`, `0x00597D50`, `0x00597D60`, `0x00597760`, `0x00597A30`.
- Ghidra read-only xrefs/callers: `get_function_callers`/`get_xrefs_to` on `0x005587F0`, `0x00558810`, `0x00558840`, `0x00559C20`, `0x005975E0`, `0x00622650`.
- Memory reads: `0x00ABDFD8` (zeroed at static load — vtable set at runtime construction, not link time), `0x0082ba60` (`"SED"`), `0x00829f7c` (`"*.%3s"`), `0x00820dac` (`"SAVEGAME.NET"`), `0x0082bbec` (`"Saving random map: %s - "`), `0x0082bc0c` (`"Loading random map: %s\n"`), `0x0082bc24` (`"lastmap.sed"`), `0x0082bc30` (`"RandMap.Sed"`), `0x007ed8e4` (vtable words).
- Existing plate comments read (not modified, not re-derived): `0x00596300` (`RandomMapSetupDialog::Proc`), `0x00596E50` (`RandomMapSetupDialog::SyncControlsFromOptions`), `0x005975E0` (`MapSeedClass::ClampFields`).
- Prior docs referenced (read, not modified): [docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_RANDOM_MAP_BEHAVIOR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_RANDOM_MAP_BEHAVIOR_GHIDRA_REPORT.md), [docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDMAP_SED_WRITER_FULL_LAYOUT_GHIDRA_REPORT.md), `docs/research/skirmish-ui/SKIRMISH_RANDOM_MAP_SETUP_DIALOG_CONTROLS_OPTIONS_GHIDRA_REPORT.md` (not opened this session — cited by parent brief as the geometry/defaults authority).
