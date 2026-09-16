# Skirmish Selected MPMode Start Launch Session - Ghidra Research Report

**Address(es):** `0x006AE2C0`, `0x006AE3F0`, `0x006ACEE0`, `0x005E7160`, `0x005D6130`, `0x005D7590`, `0x005D5B60`, `0x00686B20`, Battle-style `+0x80` body at `0x005D6BE0`, trivial accept `0x005D6310`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** selected offline Skirmish `MPModes` object propagation from Choose Map/selected-mode commit through dialog `0x102` Start `0x617`, launch/session packing, and the first selected-mode dispatch in `ScenarioClass__Full_Init`.
**Non-Scope:** multiplayer netcode/lobbies/packets, status-help/player-name edit behavior, Create Random Map internals, MIX override payload extraction, team-alliance downstream consumers, and gameplay spawn placement formulas.
**Confidence:** High for selected-mode pointer/id propagation and Rust mismatch; Medium for exact future Rust enum shape because this report is a handoff, not a Rust design.
**Active in YR:** Yes for standard offline Skirmish mode rows and Start/session handoff. Conditional for per-mode vtable side effects depending on selected mode.

## 0. Working Notes

- Target question: Does gamemd.exe launch offline Skirmish using the currently selected MPMode object, and is Rust's `SkirmishLaunchMode::Battle` hardcode a mismatch?
- Non-goals: Multiplayer netcode/lobbies/packets, status-help text, player-name edit behavior, broad shell UI research, and Rust implementation changes.
- Evidence needed to mark COMPLETE: prior docs checked, Rust current surfaces scanned, Ghidra path from dialog `0x102` Start through selected mode/session/full-init verified with caller/xref or assembly-backed evidence, and Rust handoff written.
- Stop conditions: no un-deferred open questions inside this selected-mode launch slice, or a read-only Ghidra boundary/runtime limitation is recorded as Remaining Uncertainty.

## 1. Overview

Gamemd does not collapse offline Skirmish launch to a hardcoded Battle mode. The selected `MPModes` object pointer is stored in `DAT_00A8B23C`, its numeric row id is mirrored in `DAT_00A8B250`, Start `0x617` calls the selected object's vtable `+0x14`, and `ScenarioClass__Full_Init` later dispatches the selected object's vtable `+0x80`.

Current Rust's `SkirmishLaunchMode::Battle` hardcode is therefore a parity mismatch once non-Battle mode rows are selectable. It loses at least the selected mode id/object identity, and prevents selected-mode launch/session behavior from being modeled or tested.

## 2. Class Layout / Key Offsets

| Field / global | Type | Purpose | Evidence | Active in YR |
|---|---|---|---|---|
| mode object `+0x20` | string/object field | Display text used for `0x6EB` mode rows | `0x005D6130` decompile; row add path before item-data store | Yes |
| mode object `+0x28` | `i32` | Numeric `MPModesMD.ini` row id; copied to `DAT_00A8B250` | `0x005E736D..0x005E7376` | Yes |
| mode object `+0x2C` | string/object field | Override filename passed into common constructor | `0x005D5B60`; `0x005D7590` row parse/factory path | Yes |
| mode object `+0x30` | string/object field | Map filter string used by Choose Map filtering | prior [SKIRMISH_MPMODES_SESSION_PACKING_BROAD_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MPMODES_SESSION_PACKING_BROAD_RECHECK_GHIDRA_REPORT.md); caller `0x005D6419` | Yes |
| mode object `+0x34` | bool-like | Random-map allowed row field | `0x005D7590`; `ini/mpmodesmd.ini` row field 5 | Yes |
| mode object `+0x3C` | byte | `AlliesAllowed` common override field | `0x005D5BF2`, `0x005D5CDF` | Conditional by selected mode |
| mode object `+0x3F` | byte | `MustAlly` common override field; cleared if `AlliesAllowed` is false | `0x005D5BF6`, `0x005D5CF7..0x005D5D11` | Conditional by selected mode |
| `DAT_00A8B23C` | pointer | Current selected `MPModes` object | commit at `0x005E7367`; Start call at `0x006AD2BA`; Full_Init dispatch in `0x00686B20` | Yes |
| `DAT_00A8B250` | `i32` | Current selected mode id mirror | commit at `0x005E7376`; Start mirror at `0x006AD356..0x006AD35E` | Yes |
| `DAT_00A8B254` | `i32` | Current selected map/scenario index | commit at `0x005E7370/0x005E7388`; Start mirror at `0x006AD34B..0x006AD36B` | Yes |
| `DAT_00A8B3C4` | `i32` | Launch mirror of selected mode id/token | `0x006AD356..0x006AD35E` copies `DAT_00A8B250` | Yes |
| `DAT_00A8B3C8` | `i32` | Launch mirror of selected map index, clamped to `0` if out of range | `0x006AD34B..0x006AD36B` | Yes |

## 3. Core Logic

### 3.1 `MPModesMD.ini` creates selectable mode objects

`0x005D7CE0` registers the standard mode categories `Battle`, `ManBattle`, `Siege`, `Unholy`, `FreeForAll`, and `Cooperative`. `0x005D7590` opens `MPModesMD.ini`, parses rows into object factories, and inserts created objects into the global mode vector sorted by numeric id. Active in YR: Yes. Evidence: string `MPModesMD.ini @ 0x00830A18`; category strings `0x00830BCC..0x00830C00`; decompile `0x005D7CE0`, `0x005D7590`.

The stock local roster in `ini/mpmodesmd.ini` exposes ids `1..9`: Battle, Free For All, Cooperative, Unholy Alliance, Megawealth, Duel, Meat Grind, Naval War, and Team Game. No stock Siege row is present even though the binary has Siege class support. Active in YR: Yes for rows `1..9`; No for stock offline Siege selection. Evidence: `ini/mpmodesmd.ini`; binary reader `0x005D7590`.

### 3.2 Choose Map stores object identity, not just text

The mode listbox `0x6EB` is populated by `0x005D6130`. For each accepted mode object, the code adds the visible row text, selects the row whose `mode+0x28` equals the requested id, then stores the mode object pointer as item data using message `0x19A`. Active in YR: Yes for offline Skirmish; evidence: decompile `0x005D6130`, item-data path `SendMessageA(..., 0x19A, row, mode_ptr)`.

Choose Map accept `0x005E7160` reads the selected row from `0x6EB`, obtains its item data with message `0x199`, temporarily assigns it to `DAT_00A8B23C` for validation/callbacks, then commits it. The critical commit sequence is:

```text
0x005E7367: DAT_00A8B23C = selected_mode_ptr
0x005E736D: EAX = selected_mode_ptr[10]     ; object +0x28 mode id
0x005E7370: DAT_00A8B254 = selected_map_index
0x005E7376: DAT_00A8B250 = EAX              ; selected mode id
0x005E737F: call selected_mode->vtable+0x20
0x005E7388: DAT_00A8B254 = selected_map_index again
```

Active in YR: Yes. Evidence: decompile `0x005E7160`; assembly context `0x005E734F..0x005E7388`.

### 3.3 Dialog `0x102` Start reaches the selected object

`0x006AE2C0` creates the standard Skirmish dialog `0x102` with callback `0x006AE3F0`:

```text
0x006AE31C: EDX = 0x006AE3F0
0x006AE321: ECX = 0x102
0x006AE328: call 0x00622650
```

The modal loop exits only when the local result becomes `0x617` or `0x5C0`, and returns true only for `0x617`. Active in YR: Yes for offline Skirmish setup. Evidence: decompile `0x006AE2C0`; assembly context `0x006AE31C..0x006AE37B`.

The dialog proc `0x006AE3F0` routes `WM_COMMAND (0x111)` into `0x006ACEE0`. Active in YR: Yes. Evidence: decompile `0x006AE3F0`, `param_2 == 0x111` branch calls `FUN_006ACEE0`.

Start command `0x617` in `0x006ACEE0` first performs dialog-level validations, then reads `DAT_00A8B23C` and calls the selected object's vtable `+0x14` with an initialized local output buffer:

```text
0x006AD2BA: ESI = DAT_00A8B23C
0x006AD2C4: initialize output buffer
0x006AD2C9: EAX = [ESI]                     ; selected mode vtable
0x006AD2D2: call dword ptr [EAX + 0x14]
0x006AD2D5: test AL, AL
0x006AD2D7: if true, continue to packing
0x006AD2D9: if false, compare output dword to 0x617
0x006AD2E1: false + non-0x617 continues via 0x005D5E10
0x006AD2E9..0x006AD343: false + 0x617 shows modal/re-enables Start/returns
```

Active in YR: Yes; concrete result is Conditional by selected mode. Evidence: decompile `0x006ACEE0`; assembly context `0x006AD2BA..0x006AD34B`.

### 3.4 Start packing mirrors the selected mode id

After the selected-mode `+0x14` gate, Start packing copies the committed selected mode id into launch state:

```text
0x006AD34B: EAX = DAT_00A8B254              ; selected map index
0x006AD356: EDX = DAT_00A8B250              ; selected mode id
0x006AD35E: DAT_00A8B3C4 = EDX
0x006AD364: DAT_00A8B3C8 = EAX
0x006AD36B: if map index out of range, DAT_00A8B3C8 = 0
```

Active in YR: Yes. Evidence: decompile `0x006ACEE0`; assembly context `0x006AD34B..0x006AD36B`.

The same Start branch then packs local/AI row data, random assignments, options, forced launch-state flags, and preview teardown before writing the modal result. Those details are covered by the prior session-packing reports; this report only relies on the selected-mode id/object portion. Active in YR: Yes. Evidence: `0x006AD3C1..0x006AD8D5`; prior reports listed under Sources.

### 3.5 Full_Init uses the selected object again

In non-campaign scenario initialization, `ScenarioClass__Full_Init @ 0x00686B20` reads map waypoints, creates houses, computes map bounds, then calls the selected object's vtable `+0x80` through `DAT_00A8B23C`. If `DAT_00A8B244 == 2`, it then calls `ScenarioClass__AssignStartingPoints`; otherwise it dispatches selected-mode vtable `+0x84`. Active in YR: Yes for offline Skirmish as a non-campaign mode; evidence: decompile `0x00686B20`, including `g_GameMode != 0` branch and later explicit `g_GameMode == 5` checks.

The Battle-style `+0x80` target at `0x005D6BE0` was not modeled as a Ghidra function boundary in this read-only session, so no function was created. Assembly is sufficient for the narrow claim: it calls `ScenarioClass__Gather_Start_Positions @ 0x00688380`, loops houses, reads `House+0x16058`, skips explicit value `-2`, and writes `ScenarioClass+0x1180 + start_index*4 = house_index`.

```text
0x005D6BEC: call 0x00688380
0x005D6C12: ESI = [house + 0x16058]
0x005D6C18: compare ESI, -2
0x005D6C2F: [ScenarioClass + 0x1180 + ESI*4] = house index
```

Active in YR: Yes for Battle/ManBattle-style selected modes; Conditional for other selected modes because `Full_Init` dispatches their own `+0x80` targets. Evidence: vtable/assembly context `0x005D6BE0..0x005D6C63`; `ScenarioClass__Full_Init @ 0x00686B20`.

## 4. INI Keys

| INI source | Key / rows | Native role | Evidence | Active in YR |
|---|---|---|---|---|
| `ini/mpmodesmd.ini` | `[Battle] 1`, `[Battle] 9`, `[ManBattle] 5..8`, `[FreeForAll] 2`, `[Unholy] 4`, `[Cooperative] 3` | Selectable stock offline mode roster with id, UI key, tooltip key, override filename, map filter, and random-map flag | file contents; binary reader `0x005D7590` | Yes |
| `ini/mpmodesmd.ini` | no `[Siege]` stock row | Siege class exists in binary but is not stock-selectable offline | file contents; category registration `0x005D7CE0` | No for stock offline selection |
| mode override `[MultiplayerDialogSettings]` | `WonlineTournamentAllowed`, `WonlineClanTournamentAllowed`, `AlliesAllowed`, `MustAlly` | Common object override fields read by constructor | `0x005D5B60`; reads at `0x005D5CA7..0x005D5D11` | Yes / Conditional by mode |
| `rulesmd.ini [MultiplayerDialogSettings]` | money/unit/speed/checkbox defaults | Dialog control defaults later packed on Start; not selected-mode object identity | prior session-packing report; Start reads controls at `0x006AD703..0x006AD889` | Yes |

## 5. Integration Points

| Integration | Behavior | Evidence | Active in YR |
|---|---|---|---|
| Mode roster load | `MPModesMD.ini` rows create sorted mode objects | `0x005D7590`; `0x005D7CE0`; `ini/mpmodesmd.ini` | Yes |
| Mode list `0x6EB` | row item data stores object pointer | `0x005D6130` | Yes |
| Choose Map accept | selected object pointer, selected mode id, and map index commit together | `0x005E7160`; `0x005E734F..0x005E7388` | Yes |
| Dialog `0x102` launcher | creates setup dialog with proc `0x006AE3F0`; returns true only after result `0x617` | `0x006AE2C0`; `0x006AE31C..0x006AE37B` | Yes |
| Start `0x617` | calls selected object vtable `+0x14` before packing | `0x006ACEE0`; `0x006AD2BA..0x006AD34B` | Yes / Conditional by mode |
| Start session mirror | copies `DAT_00A8B250` into `DAT_00A8B3C4`; copies/clamps `DAT_00A8B254` into `DAT_00A8B3C8` | `0x006AD34B..0x006AD36B` | Yes |
| Scenario full init | dispatches selected object vtable `+0x80`, then assignment path or selected object `+0x84` | `0x00686B20`; Battle-style body `0x005D6BE0` assembly | Yes / Conditional by mode |

No simulation tick-cycle behavior is claimed here. This is shell/session startup and first scenario initialization handoff.

## 6. Current Rust Implementation Status

Rust already has a partial mode data model:

- `src/skirmish_modes.rs:21` defines `SkirmishGameMode` with id, UI key, tooltip key, override filename, map filter, random-map flag, `allies_allowed`, and `must_ally`.
- `src/skirmish_modes.rs:83` parses `ini/mpmodesmd.ini`; `src/skirmish_modes.rs:105` exposes `stock_skirmish_modes`.
- `src/ui/skirmish_shell/state.rs:159` stores `ChooseMapModalState::selected_mode_id`; `state.rs:819` stores `SkirmishShellState::selected_mode_id`.
- Choose Map filtering tests exist around `state.rs:2205..2290`.

The launch handoff is still Battle-only:

- `src/skirmish_launch.rs:14` defines `SkirmishLaunchMode` with only `Battle`.
- `src/ui/skirmish_shell/state.rs:1914` `launch_session(state, maps)` does not receive the modes list and cannot look up the selected mode object/id.
- `src/ui/skirmish_shell/state.rs:1994..1996` returns `SkirmishLaunchSession { mode: SkirmishLaunchMode::Battle, ... }` regardless of `state.selected_mode_id`.
- `src/ui/skirmish_shell/state.rs:3039..3041` has a test asserting every launch session is Battle.
- `src/app.rs:623` calls `launch_session(&state.skirmish_shell_state, &state.skirmish_shell_maps)` and routes the resulting session into `pending_skirmish_launch_session`.
- `src/app_skirmish.rs:162` applies `SkirmishLaunchSession` to houses/spawns, but the current code path does not branch on `session.mode`.

Current Rust delta: mismatch. A user can select visible non-Battle stock modes, but the launched session cannot carry or consume that selected mode identity. Active in YR: Yes; evidence: mode commit and Start/Full_Init selected vtable dispatch listed above.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Prior MPModes/session reports | verified for reuse | four named prior reports in Sources | none for target-critical facts |
| `MPModesMD.ini` roster load | verified | `0x005D7590`, `0x005D7CE0`, `ini/mpmodesmd.ini` | MIX override payloads owned by slot 2 |
| Common constructor selected-mode fields | verified | `0x005D5B60`, `0x005D5CA7..0x005D5D11` | full override payload extraction out of scope |
| Mode list `0x6EB` item data | verified | `0x005D6130` | none for object identity |
| Choose Map selected-mode commit | verified | `0x005E7160`; `0x005E734F..0x005E7388` | none |
| Dialog `0x102` creation | verified | `0x006AE2C0`; `0x006AE31C..0x006AE328` | none |
| Dialog proc command routing | verified | `0x006AE3F0` | none |
| Start selected `+0x14` call | verified | `0x006ACEE0`; `0x006AD2BA..0x006AD34B` | selected-mode validation details owned by slot 4 |
| Start selected id mirror | verified | `0x006AD34B..0x006AD36B` | none |
| Full_Init selected `+0x80/+0x84` dispatch | verified | `0x00686B20` decompile | exact non-Battle `+0x80` bodies out of scope |
| Battle-style `+0x80` body | verified by assembly, no function boundary created | `0x005D6BE0..0x005D6C63` | none for proving selected dispatch is not Rust Battle hardcode |
| Current Rust selected mode model | verified | `src/skirmish_modes.rs`; `src/ui/skirmish_shell/state.rs` | no Rust edits in this slot |
| Current Rust launch hardcode | verified mismatch | `src/skirmish_launch.rs:14`; `src/ui/skirmish_shell/state.rs:1994` | implementation needed |
| Multiplayer netcode/lobbies/packets | deferred | user non-scope | separate investigation if requested |
| Gameplay spawn placement formulas | deferred | user non-scope | use spawn/assignment reports |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-01 - Which mode object is selected before Start? -> the `0x6EB` item-data object pointer is committed to `DAT_00A8B23C`; its `+0x28` id is copied to `DAT_00A8B250`.` (evidence: `0x005E7160`; `0x005E734F..0x005E7388`)
- `[RESOLVED] OQ-02 - Does Start `0x617` use `DAT_00A8B23C` or hardcoded Battle? -> it reads `DAT_00A8B23C` and calls selected vtable `+0x14`.` (evidence: `0x006AD2BA..0x006AD34B`)
- `[RESOLVED] OQ-03 - Is selected mode id packed for launch? -> yes, `DAT_00A8B250` is copied to `DAT_00A8B3C4` after selected-mode acceptance.` (evidence: `0x006AD34B..0x006AD36B`)
- `[RESOLVED] OQ-04 - Does Full_Init still use selected mode after the shell exits? -> yes, non-campaign `Full_Init` dispatches `DAT_00A8B23C` vtable `+0x80`, then either `AssignStartingPoints` or selected vtable `+0x84`.` (evidence: `0x00686B20`)
- `[RESOLVED] OQ-05 - What does Battle-style `+0x80` do for start preassignment? -> calls `Gather_Start_Positions`, reads `House+0x16058`, skips `-2`, writes `ScenarioClass+0x1180[start]=house_index`.` (evidence: `0x005D6BE0..0x005D6C63`)
- `[RESOLVED] OQ-06 - Which stock rows make Battle-only launch visibly wrong? -> ids `2..9` can be selected and should remain distinguishable through launch; Team Game id `9` and FFA id `2` have known mode-specific shell/session constraints.` (evidence: `ini/mpmodesmd.ini`; `0x005D7590`; `0x005E7160`)
- `[RESOLVED] OQ-07 - Is Siege stock-selectable? -> no stock offline row, even though binary class registration exists.` (evidence: `ini/mpmodesmd.ini`; `0x005D7CE0`)
- `[RESOLVED] OQ-08 - Does current Rust have a selected mode in launch session? -> no; `SkirmishLaunchMode` only has `Battle`, and `launch_session` hardcodes it.` (evidence: `src/skirmish_launch.rs:14`; `src/ui/skirmish_shell/state.rs:1994`)
- `[RESOLVED] OQ-09 - Does current Rust `launch_session` have enough input to look up the selected mode object? -> no; it accepts only state and maps, not the mode roster/object table.` (evidence: `src/ui/skirmish_shell/state.rs:1914`)
- `[RESOLVED] OQ-10 - Is this networking/lobby session behavior? -> no; the verified path is offline dialog `0x102`, local globals, and `ScenarioClass__Full_Init`.` (evidence: `0x006AE2C0`, `0x006ACEE0`, `0x00686B20`)
- `[DEFERRED] OQ-11 - Exact non-Battle selected-mode `+0x80` bodies.` (category: out-of-scope; reason: target only needs to prove selected-mode dispatch and Rust handoff; separate mode-specific reports should own per-mode spawn/start semantics; next-step-if-pursued: resolve vtable `+0x80/+0x84` for FreeForAll/Coop/Unholy/ManBattle)
- `[DEFERRED] OQ-12 - MIX-backed override payload extraction for all mode INIs.` (category: out-of-scope; reason: slot 2 owns override payloads; next-step-if-pursued: consume that report for mode object construction)
- `[DEFERRED] OQ-13 - Team alliance downstream effects from selected Team Game.` (category: out-of-scope; reason: slot 5 owns team adjunct/house alliance handoff; next-step-if-pursued: consume that report for acceptance tests)

Adversarial corner-case checks:

- If selected mode is Team Game id `9`, Start should not silently become Battle id `1`; evidence says `DAT_00A8B250` id `9` is copied to `DAT_00A8B3C4`.
- If selected map index is invalid at Start, only the map mirror clamps to `0`; selected mode id still comes from `DAT_00A8B250`.
- If selected-mode `+0x14` returns false with output not equal to `0x617`, Start continues through `0x005D5E10`; do not equate every false return with hard failure.
- If the selected mode pointer changes in Choose Map and the dialog is accepted, `DAT_00A8B23C` changes before Start; if not accepted, this report does not claim a commit.
- If Ghidra lacks a function boundary at Battle-style `0x005D6BE0`, read-only assembly still proves the selected-mode `+0x80` target body without creating a function.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Selected mode object pointer and id are committed together by Choose Map; Start uses that committed selected object. | `0x005E734F..0x005E7388`; `0x006AD2BA..0x006AD34B` | mismatch: `launch_session` ignores `selected_mode_id` and always returns Battle | `src/ui/skirmish_shell/state.rs::launch_session`, `src/skirmish_launch.rs::SkirmishLaunchSession` | Carry selected mode id/object data into the launch session, not just map file/options. | Select Team Game id `9`, press Start, and assert the launch session records id `9` / Team Game metadata rather than Battle. | Do not derive launch mode from display text; use parsed mode id/object. |
| Start packing mirrors selected mode id into launch state before leaving the dialog. | `0x006AD34B..0x006AD36B` | missing: Rust has no numeric selected mode id in `SkirmishLaunchSession` | `src/skirmish_launch.rs`, app pending-launch routing in `src/app.rs` | Add a deterministic selected-mode launch field that survives pending launch and app init. | Start with FFA id `2`; pending session passed through `state.pending_skirmish_launch_session` still identifies id `2`. | Do not keep an enum with only `Battle` as the sole launch truth. |
| `ScenarioClass__Full_Init` dispatches selected mode vtable `+0x80/+0x84`; Battle-style body is only one selected-mode target. | `0x00686B20`; `0x005D6BE0..0x005D6C63` | missing: `app_skirmish.rs` does not branch on `session.mode` | `src/app_skirmish.rs::apply_skirmish_launch_session`; future selected-mode behavior module | Keep mode-specific launch/start-assignment hooks possible; implement Battle-style behavior as one case, not as global behavior. | Non-Battle session reaches a mode-specific acceptance/start-assignment test hook instead of falling through indistinguishably as Battle. | Do not make Battle start preassignment the unconditional path for all selected modes without verifying each mode. |
| Stock selectable mode rows are data-driven ids `1..9`; Siege exists in binary but not stock row data. | `ini/mpmodesmd.ini`; `0x005D7590`; `0x005D7CE0` | partial: Rust parses ids `1..9`, but launch hardcode discards them | `src/skirmish_modes.rs`, `src/ui/skirmish_shell/state.rs`, `src/skirmish_launch.rs` | Reuse parsed `SkirmishGameMode` data for launch; preserve no stock Siege row. | `stock_skirmish_modes()` launches every visible stock id with the same id selected; no Siege session appears from stock data. | Do not synthesize stock Siege just because the binary registers a class. |
| Selected-mode `+0x14` is called before packing; only false plus output dword `0x617` blocks via the generic modal path. | `0x006AD2BA..0x006AD34B`; prior vtable acceptance report | missing/unchecked: Rust has ordinary validation but no selected-mode acceptance model | future `launch_session` selected-mode validation surface | Add a selected-mode acceptance step once mode-specific validation is modeled; keep ordinary capacity/min-player/same-team checks separate. | Synthetic test mode with false+`0x617` blocks before pending session; stock Battle/ManBattle accept. | Do not implement "any false return blocks" or fold mode acceptance into networking/lobby code. |
| Mode-specific fields such as `MustAlly` are attached to selected mode object and affect shell/session constraints. | `0x005D5B60`; `0x005D5CF7..0x005D5D11`; prior team combo report | partial/mismatch: team rows and launch validation do not fully consume selected mode object | `src/skirmish_modes.rs`; team combo builder; `launch_session` validation | Pass selected mode into team-row/launch validation so Team Game and FFA constraints survive into session. | Select Team Game id `9`; Team None is unavailable/rejected and launch still records id `9`. | Do not treat all modes as Battle with only a map filter difference. |

## Negative Facts / Do Not Do

- Do not hardcode `SkirmishLaunchMode::Battle` as the only launched offline Skirmish mode. Active in YR: Yes for ids `2..9`; evidence `ini/mpmodesmd.ini`, selected commit `0x005E734F..0x005E7388`, Start `0x006AD2BA`.
- Do not treat the selected mode as UI-only state. Active in YR: Yes; evidence Start copies `DAT_00A8B250` into `DAT_00A8B3C4` and `Full_Init` dispatches `DAT_00A8B23C`.
- Do not use map filter, visible label, or category name as the launch identity. Active in YR: Yes; evidence object field `+0x28` is the id copied to `DAT_00A8B250`.
- Do not expose stock Siege in offline Skirmish from class registration alone. Active in YR: No for stock roster; evidence `ini/mpmodesmd.ini` has no Siege row.
- Do not conflate mode acceptance with multiplayer networking. Active in YR: Yes for offline dialog `0x102`; evidence `0x006AE2C0`, `0x006ACEE0`.
- Do not assume every selected-mode `+0x14` false return blocks Start. Active in YR: Yes; blocking generic path requires false plus output dword `0x617`.
- Do not create missing Ghidra function boundaries in this context. Active in YR: n/a; read-only constraint satisfied by assembly inspection at `0x005D6BE0`.

## Sources

- Ghidra read-only decompile: `0x006AE2C0`, `0x006AE3F0`, `0x006ACEE0`, `0x005E7160`, `0x005D6130`, `0x005D7590`, `0x005D5B60`, `0x005D7CE0`, `0x00686B20`, `0x005D6310`.
- Ghidra assembly contexts: `0x006AE31C..0x006AE37B`, `0x005E734F..0x005E7388`, `0x006AD2BA..0x006AD34B`, `0x006AD34B..0x006AD36B`, `0x005D6BE0..0x005D6C63`.
- Prior reports checked: [docs/research/skirmish-ui/SKIRMISH_MPMODES_SESSION_PACKING_BROAD_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_MPMODES_SESSION_PACKING_BROAD_RECHECK_GHIDRA_REPORT.md), [SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_HANDOFF_SESSION_PACKING_GHIDRA_REPORT.md), [SKIRMISH_START_SESSION_VTABLE_0X14_ACCEPTANCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_SESSION_VTABLE_0X14_ACCEPTANCE_GHIDRA_REPORT.md), [SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_START_GAME_TO_SPAWN_CONSUMERS_GHIDRA_REPORT.md).
- INI checked: `ini/mpmodesmd.ini`, `ini/rulesmd.ini`.
- Rust scanned: `src/skirmish_modes.rs`, `src/skirmish_launch.rs`, `src/ui/skirmish_shell/state.rs`, `src/app.rs`, `src/app_skirmish.rs`, `src/app_init.rs`, `src/app_transitions.rs`.
