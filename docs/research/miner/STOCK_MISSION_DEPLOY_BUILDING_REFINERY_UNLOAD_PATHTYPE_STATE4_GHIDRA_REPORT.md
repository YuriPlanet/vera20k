# Stock Mission_Deploy_Building Refinery Unload PathType And State 4 - Ghidra Research Report

**Address(es):** `0x0073D630` primary, `0x0065AE30` PathType helper, `0x004595C0` release helper, `0x0047C520` building lookup, `0x0049F2F0` adjacent-offset init
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** stock `CMIN/HARV -> GAREFN/NAREFN` zero-link refinery unload branch in `UnitClass::Mission_Deploy_Building`, including entry split on `unit+0x2E4`, `SizeLimit` versus `Harvester=yes` routing, `RadioClass::In_Radio_Contact` (formerly mislabeled PathType__Has_Valid_Steps) guard polarity, state 3 empty-cargo transition, state 4 direct returns versus timer epilogue, `+0x6D1` clear, `building+0x57C` wait, `SetMission(0x0A)`, `QueueMission`, radio `3`, and normal-path reachability of `ReleaseDockedHarvester` / `Force_Track(0x47)`.
**Non-Scope:** two-miner queue handoff timing, full Mission_Harvest retry loop, full runtime tick-order proof outside this function, modded refinery `ProductionAnim` lifetime beyond identifying the wait guard, and Yuri slave miner.
**Confidence:** High for the claimed slice.
**Active in YR:** Yes for stock HARV/CMIN unloading at GAREFN/NAREFN; conditional branches are marked per finding.

## 1. Overview

The stock refinery unload path is the zero-`unit+0x2E4` path. It does not require a reciprocal `unit/building +0x2E4` dock link, and normal stock cargo-empty completion does not call `BuildingClass::ReleaseDockedHarvester`.

The key correction is the `RadioClass::In_Radio_Contact` guard polarity. At `0x0073DEE2`, a true result jumps to the timer/state-dispatch path; false takes a cleanup branch that can direct-return `1`. Therefore the prior RED doc's "steps present -> return 5" reading is inverted.

## 2. Class Layout / Key Offsets

| Field / offset | Meaning in this slice | Evidence | Active in YR |
|---|---|---|---|
| Unit `+0x2E4` / `param_1[0xB9]` | top-level reciprocal dock-link branch selector | `0x0073D63B CMP [ESI+0x2E4],0`; `0x0073D641 JZ 0x0073D6E6` | Conditional; stock refinery normal path keeps this zero |
| Unit `+0x6C4` / `param_1[0x1B1]` | `UnitTypeClass*` | decompile `0x0073D630` | Yes |
| UnitType `+0x5E0` | `SizeLimit` | `0x0073D6EC CMP [EAX+0x5E0],0` | Yes, but stock HARV/CMIN default to no key / zero |
| UnitType `+0xE0E` | `Harvester=yes` | `0x0073D678`, `rulesmd.ini:[CMIN]/[HARV] Harvester=yes` | Yes |
| UnitType `+0xE0F` | `Weeder=yes` | `0x0073D686`; no stock CMIN/HARV weeder | No for stock YR miners |
| Unit `+0xBC` / `param_1[0x2F]` | unload/deploy mission substate | state writes at `0x0073E093`, `0x0073E51C`, `0x0073E594` | Yes |
| Unit `+0xF8` / `param_1[0x3E]` | dump-rate accumulator | gate at `0x0073E35B..0x0073E374`; reset at `0x0073E4D0` | Yes |
| Unit byte `+0x6D1` | unload-active / first-entry flag | set `0x0073DFDA`, clear `0x0073E1F6` | Yes |
| Unit `+0x5A4` | destination/contact pointer used in override checks | reads `0x0073E1F0`, `0x0073E539` | Yes |
| Unit `+0xB4` / `param_1[0x2D]` | queued mission id | reads `0x0073E201`, `0x0073E543`; `MissionClass__Queue_Mission @ 0x005B35E0` writes `param_1[0x2D]` | Yes |
| Unit `+0x33C` | harvester `StorageClass` | `StorageClass__FindFirstNonEmptySlot` at `0x0073E3BF` | Yes |
| BuildingType `+0x16B3` | `DockUnload=yes` | `BuildingClass::Receive_Radio @ 0x0043C2D0` case `0x15` | Yes for GAREFN/NAREFN |
| BuildingType `+0x16BB` | `Refinery=yes` | state 4 guard `0x0073E1D5`; `rulesmd.ini` | Yes for GAREFN/NAREFN |
| Building `+0x57C` | `Anims_0[8]` / live `ProductionAnim` pointer | `0x0073E1DF`; [BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_GHIDRA_REPORT.md) | Conditional; stock GAREFN/NAREFN normally keep it null |
| Building `+0x584` | slot 10 `SpecialAnim` pointer | reads `0x0073E384`, `0x0073E526` | Yes |
| `g_refinery_unload_adjacent_lookup_dx/dy` | signed west-neighbor lookup `(-1,0)` | init `0x0049F2F0`, uses `0x0073E195`, `0x0073E2D5` | Yes |

## 3. Core Logic

### 3.1 Entry split on `unit+0x2E4`

The function starts:

```text
0x0073D63B  CMP [ESI+0x2E4], 0
0x0073D641  JZ  0x0073D6E6
```

So:

- `unit+0x2E4 == 0` enters the normal deploy/refinery FSM.
- `unit+0x2E4 != 0` falls through to cell lookup and `BuildingClass::ReleaseDockedHarvester @ 0x004595C0`.

**Active in YR:** Yes. The mission handler is live. The nonzero branch is conditional on a writer setting `unit+0x2E4`; the stock GAREFN/NAREFN path does not do that according to the writer inventory.

### 3.2 `SizeLimit` does not exclude stock harvesters

At `0x0073D6E6`, the zero-link path reads `UnitType+0x5E0`:

```text
0x0073D6EC  CMP [UnitType+0x5E0], 0
0x0073D6F2  JLE 0x0073DCD3
```

The `JLE` path is not "non-harvester only." It reaches `LAB_0073D672`, which then tests `UnitType+0xE0E` and `+0xE0F`; stock HARV/CMIN have `Harvester=yes`, so they enter `0x0073DEE0`.

**Active in YR:** Yes. `rulesmd.ini` has no active `SizeLimit` key under `[CMIN]` or `[HARV]`; both have `Harvester=yes`.

### 3.3 `RadioClass::In_Radio_Contact` helper semantics

`RadioClass__In_Radio_Contact @ 0x0065AE30` scans `param_1+0xE4` for `param_1+0xE8` entries and returns true if any entry is nonzero. Empty count or all-zero entries return false.

The first harvester-path guard is:

```text
0x0073DEE2  CALL 0x0065AE30
0x0073DEE7  TEST AL, AL
0x0073DEE9  JNZ 0x0073DF56
```

Therefore:

- true / valid steps present -> proceed to RateTimer/facing/state dispatch at `0x0073DF56`;
- false / no valid steps -> take cleanup branch at `0x0073DEEB`.

The false branch calls unit vtable `+0x484`, clears `unit+0x6D1`, optionally calls unit vtable `+0x500`, checks vtable `+0x200`, optionally queues a mission, and direct-returns `1`. It does not return `5`.

**Active in YR:** Yes for every stock unload tick.

### 3.4 RateTimer gate and direct `return 5`

When path steps are valid, the function reads the facing/rate timer:

```text
0x0073DF66..0x0073DF72  (((timer >> 7) + 1) & 0x1FE) == 0x80
0x0073DF78              JZ 0x0073DFBD
```

If the timer window is not ready, and byte `unit+0x6AF` is false, the function asks the locomotor to face `0x4000`, then direct-returns `5` at `0x0073DFB3`.

**Active in YR:** Yes. This is the direct `return 5` path the RED doc misplaced onto the PathType branch.

### 3.5 State 3 init

When `unit+0x6D1 == 0`, the function:

1. resets `unit+0xF8 = 0`;
2. sets `unit+0x6D1 = 1`;
3. initializes the periodic accumulator block at `+0x100..+0x10C`;
4. if `Harvester=yes`, looks up the refinery at current cell plus `(-1,0)`;
5. calls `SetAnimSlotImage(slot 7)` if a building is found;
6. writes `unit+0xBC = 3`;
7. falls to the timer epilogue at `0x0073E289`.

**Active in YR:** Yes. Slot 7 is a no-op for stock GAREFN/NAREFN because they do not define `PreProductionAnim`, but the call site is active.

### 3.6 State 3 deposit loop and empty-cargo transition

State 3 re-finds the refinery by adjacent-cell lookup and checks:

```text
HarvesterDumpRate * 900.0 <= unit+0xF8
```

Default `HarvesterDumpRate=0.016`, so the threshold is `14.4` frames. On threshold crossing, it emits building particles, possibly starts slot 10 `SpecialAnim`, finds the first non-empty storage slot, and drains the whole slot.

If `FindFirstNonEmptySlot` returns `-1`, or `RemoveAmount` returns no positive amount, the code:

```text
0x0073E513  PUSH 0x8
0x0073E517  CALL 0x00451750       ; SetAnimSlotImage(slot 8)
0x0073E51C  MOV [ESI+0xBC], 4
0x0073E530  PUSH 0xA
0x0073E534  CALL 0x00451E40       ; ClearAnimSlot(slot 10) if occupied
0x0073E539  ... override check
0x0073E5B4  MOV EAX, 1
0x0073E5BD  RET
```

So the state 3 -> state 4 empty-cargo transition direct-returns `1`, not the timer epilogue.

**Active in YR:** Yes. The slot-8 call is active but normally no-op for stock refineries; the state write and direct return are live.

### 3.7 State 4 non-Weeder normal exit

For stock non-Weeder HARV/CMIN, state 4 starts at `0x0073E17F`:

1. find current cell plus `(-1,0)`;
2. call `Look_up_building_in_cell`;
3. if building exists, `Refinery=yes`, and `building+0x57C != 0`, direct-return `1` at `0x0073E5B1`;
4. otherwise clear `unit+0x6D1 = 0`;
5. inspect override state: `unit+0x5A4`, queued mission `+0xB4`, and mission id `0x0A`;
6. on normal stock exit, call vtable `+0x1E8` with mission `0x0A` and queued flag `0`;
7. if vtable `+0x200` succeeds, call `RadioClass::In_Radio_Contact`;
8. if true, send radio command `3` via vtable `+0x274`;
9. call vtable `+0x1EC` / `QueueMission`;
10. fall through to timer epilogue at `0x0073E289`.

Normal stock exit therefore uses timer-epilogue return after `SetMission(Harvest=0x0A)` and `QueueMission`, unless the slot-8 wait guard direct-returns `1` first.

**Active in YR:** Yes. The state-4 branch is active for stock miners. The `building+0x57C` wait is normally inactive for stock GAREFN/NAREFN because slot 8 `ProductionAnim` is not defined, but it is active for mods or any rules/art that populates slot 8.

### 3.8 Timer epilogue is not all-path convergence

Timer epilogue:

```text
0x0073E28B  MissionClass__GetMissionTimerEntry
0x0073E290  FLD [entry+0x10]
0x0073E293  FMUL 900.0
0x0073E299  Math__ftol
0x0073E2B0  RandomRanged(0,2)
0x0073E2B5  ADD EAX, ESI
0x0073E2BE  RET
```

But direct returns exist:

- `0x0073DFB3`: return `5` when PathType is true but the RateTimer window is not ready.
- `0x0073DF49`: return `1` from the no-valid-steps cleanup branch after optional queueing.
- `0x0073E1EA -> 0x0073E5B1`: return `1` while `building+0x57C`/slot 8 is non-null.
- `0x0073E5B4`: return `1` from state 3 post-deposit/empty/override paths.

**Active in YR:** Yes.

### 3.9 ReleaseDockedHarvester / Force_Track reachability

`ReleaseDockedHarvester @ 0x004595C0` clears slots 10 and 11, may play `BunkerWallsDownSound`, creates slots 12 and 13 when defined, reads `building+0x2E4`, clears `unit+0x2E4`, calls locomotor `Force_Track` with track `0x47`, sets a destination, sets mission Move `2`, clears `building+0x2E4`, sets building mission Guard `5`, and sends radio `3`.

`Mission_Deploy_Building` calls it only at `0x0073D66D`, before the zero-link harvester FSM, and only on the nonzero `unit+0x2E4` entry branch. Standard refinery writer inventory did not find a stock GAREFN/NAREFN writer for reciprocal `+0x2E4`.

**Active in YR:** Conditional. The helper is live for reciprocal-link contexts. It is not reachable on normal stock zero-link cargo-empty completion.

## 4. INI Keys

| INI key | Stock value | Effect in this slice | Active in YR |
|---|---|---|---|
| `rulesmd.ini:[CMIN] Dock` | `NAREFN,GAREFN` | stock chrono miner refinery candidates | Yes |
| `rulesmd.ini:[CMIN] Harvester` | `yes` | reaches `UnitType+0xE0E` harvester branch | Yes |
| `rulesmd.ini:[CMIN] Storage` | `20` | carried ore capacity; `StorageClass` source for state 3 | Yes |
| `rulesmd.ini:[CMIN] UnloadingClass` | `CMON` | display override while unloading; not a branch gate here | Yes |
| `rulesmd.ini:[CMIN] Teleporter` | `yes` | chrono movement elsewhere; not checked in this unload branch | Yes |
| `rulesmd.ini:[HARV] Dock` | `NAREFN,GAREFN` | stock war miner refinery candidates | Yes |
| `rulesmd.ini:[HARV] Harvester` | `yes` | reaches same branch as CMIN | Yes |
| `rulesmd.ini:[HARV] Storage` | `40` | carried ore capacity | Yes |
| `rulesmd.ini:[HARV] UnloadingClass` | `HORV` | display override while unloading | Yes |
| `rulesmd.ini:[GAREFN] DockUnload` | `yes` | radio case `0x15` sends sender mission `0x10` | Yes |
| `rulesmd.ini:[GAREFN] Refinery` | `yes` | state 4 guard and slot-8 completion call | Yes |
| `rulesmd.ini:[GAREFN] NumberOfDocks` | `1` | refinery admission capacity context | Yes |
| `rulesmd.ini:[GAREFN] FreeUnit` | `CMIN` | production-side free miner; not unload branch | Yes |
| `rulesmd.ini:[GAREFN] Storage` | `200` | building storage/visual context; not capacity gate in state 3 | Yes |
| `rulesmd.ini:[NAREFN] DockUnload` | `yes` | same as GAREFN | Yes |
| `rulesmd.ini:[NAREFN] Refinery` | `yes` | same as GAREFN | Yes |
| `rulesmd.ini:[NAREFN] NumberOfDocks` | `1` | same as GAREFN | Yes |
| `rulesmd.ini:[NAREFN] FreeUnit` | `HARV` | production-side free miner; not unload branch | Yes |
| `[General] HarvesterDumpRate` | `0.016` | state 3 gate: `0.016 * 900.0 = 14.4` | Yes |
| `[General] PurifierBonus` | `.25` | bonus calculation after slot drain | Yes |
| `[General] AIVirtualPurifiers` | `4,2,0` | AI bonus addend in skirmish | Yes for AI skirmish |
| `[General] ConditionYellow` | `50%` | damaged art variant selector for slots 7/8/10 | Yes |
| `artmd.ini:[GAREFN]/[NAREFN] Foundation` | `4x3` | layout context; not used by `DAT_0089F6A0` lookup | Yes |
| `artmd.ini:[GAREFN]/[NAREFN] QueueingCell` | `4,1` | queue context; not the state 3/4 refinery lookup | Yes |
| `artmd.ini:[GAREFN]/[NAREFN] SpecialAnim` | `GAREFNOR`/`NAREFNOR` | slot 10 per-dump anim | Yes |
| `artmd.ini ProductionAnim` | not active for stock GAREFN/NAREFN | would populate `building+0x57C` and make state 4 wait | Conditional / modded |

## 5. Integration Points

| Function | Role | Evidence | Active in YR |
|---|---|---|---|
| `BuildingClass::Receive_Radio @ 0x0043C2D0` | case `0x0E` admission; case `0x15` sends sender mission `0x10` for DockUnload | decompile `0x0043C2D0` | Yes |
| `UnitClass::PerCellProcess @ 0x00739EC0` | pad arrival sends radio `0x15`; does not write reciprocal `+0x2E4` | decompile `0x00739EC0` | Yes |
| `UnitClass::Mission_Deploy_Building @ 0x0073D630` | unit-side unload FSM | decompile/disassembly | Yes |
| `RadioClass::In_Radio_Contact @ 0x0065AE30` | route guard and radio-3 condition | decompile `0x0065AE30` | Yes |
| `Look_up_building_in_cell @ 0x0047C520` | scans `CellClass+0xE4` object list for `WhatAmI()==6` | decompile `0x0047C520` | Yes |
| `Foundation_direction_table_init @ 0x0049F2F0` | initializes `g_refinery_unload_adjacent_lookup_dx = 0x0000FFFF`, i.e. `(-1,0)` | decompile `0x0049F2F0` | Yes |
| `MissionClass::GetMissionTimerEntry @ 0x005B3A00` | timer epilogue entry lookup from current mission id | decompile `0x005B3A00` | Yes |
| `MissionClass::Queue_Mission @ 0x005B35E0` | queued mission field semantics; writes `param_1[0x2D]` | decompile `0x005B35E0` | Yes |
| `BuildingClass::ReleaseDockedHarvester @ 0x004595C0` | conditional reciprocal-link release and `Force_Track(0x47)` | decompile `0x004595C0` | Conditional; not stock zero-link completion |

## 6. Current Rust Implementation Status

Relevant Rust surfaces:

- `src/sim/miner/mod.rs` `RefineryDockPhase`: models `Approach`, `MissionEnter`, `AwaitingAcceptedCell`, `Linked`, `Pivoting`, `Unloading`, `DepositCooldown`, `Departing`.
- `src/sim/miner/miner_dock_sequence.rs` `phase_unloading`: drains one resource slot per dump threshold and emits one `BaleDepositEvent`.
- `src/sim/miner/miner_dock_sequence.rs` `phase_deposit_cooldown`: holds for one more dump-gate interval after the last slot drain.
- `src/sim/miner/miner_dock_sequence.rs` `phase_departing`: clears stock dock bookkeeping and returns to `SearchOre` without `Force_Track(0x47)` or release-helper effects.
- `src/sim/miner/miner_dock.rs` `RefineryDockContacts`: Rust contact/queue bookkeeping distinct from reciprocal `+0x2E4`.

Status against this report: the current comments and broad implementation direction match the corrected zero-link state-4 model. Remaining parity risk is exact timing around the state 3 empty-check tick and multi-miner queue handoff; those are assigned to the adjacent investigations, not this branch-polarity report.

No Rust files, INI files, or existing docs were modified.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Expected output existence | verified | `Test-Path` returned false before writing | none |
| Entry split on `unit+0x2E4` | verified | `0x0073D63B`, `0x0073D641` | none |
| Nonzero `unit+0x2E4` release branch | verified | `0x0073D647..0x0073D66D`; `0x004595C0` | exact non-stock runtime frequency out of scope |
| Stock no-writer evidence for `+0x2E4` | verified via prior report | [STANDARD_REFINERY_0X2E4_WRITER_INVENTORY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/STANDARD_REFINERY_0X2E4_WRITER_INVENTORY_GHIDRA_REPORT.md); spot-check `0x0043C2D0`, `0x00739EC0` | none for stock path |
| `SizeLimit` branch correction | verified | `0x0073D6EC JLE 0x0073DCD3`; harvester gate `0x0073D678` | none |
| `RadioClass::In_Radio_Contact` helper body | verified | decompile `0x0065AE30` | none |
| PathType guard polarity in primary function | verified | `0x0073DEE2..0x0073DEE9` | none |
| No-valid-steps branch return | verified | `0x0073DEEB..0x0073DF55` | concrete names for vtable `+0x484/+0x500/+0x200` not needed for this slice |
| RateTimer direct `return 5` | verified | `0x0073DF56..0x0073DFBC` | none |
| State 3 init | verified | `0x0073DFBD..0x0073E09D` | none |
| Adjacent refinery lookup | verified | use sites `0x0073E013`, `0x0073E181`, `0x0073E2C8`; helper `0x0047C520`; init `0x0049F2F0` | none |
| State 3 dump gate | verified | `0x0073E355..0x0073E374`; INI `HarvesterDumpRate=0.016` | none |
| State 3 empty transition to state 4 | verified | `0x0073E4DC..0x0073E534` | none |
| State 3 direct return `1` | verified | `0x0073E5B1..0x0073E5BD` | none |
| State 4 slot-8 wait guard | verified | `0x0073E1CB..0x0073E1EA`; `BUILDINGCLASS_0X57C...` | modded slot-8 runtime lifetime out of scope |
| State 4 `+0x6D1` clear | verified | `0x0073E1F6` | none |
| State 4 normal `SetMission(0x0A)` | verified | `0x0073E24F..0x0073E254` | none |
| State 4 optional radio `3` | verified | `0x0073E268..0x0073E279` | none |
| State 4 `QueueMission` and timer epilogue | verified | `0x0073E27F..0x0073E2BE` | none |
| `ReleaseDockedHarvester` / `Force_Track(0x47)` exclusion from normal stock completion | verified | only call at `0x0073D66D`; stock writer inventory | none |
| Two-miner queue takeover timing | deferred | not part of this report | adjacent trace/re-investigate task |

## 8. Open Questions - Final State Of The Investigation Log

- [RESOLVED] OQ-01 - What mode is this investigation? -> exhaustive-slice for the bounded `UnitClass::Mission_Deploy_Building` stock zero-link PathType/state-4 branch. (evidence: user scope and primary function boundary)
- [RESOLVED] OQ-02 - Does the expected report already exist? -> No; output path did not exist before writing. (evidence: `Test-Path docs/research/miner/STOCK_MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_PATHTYPE_STATE4_GHIDRA_REPORT.md`)
- [RESOLVED] OQ-03 - Is the stock path `unit+0x2E4 == 0` or nonzero? -> Zero enters the normal FSM; nonzero calls release. (evidence: `0x0073D63B`, `0x0073D641`)
- [RESOLVED] OQ-04 - Is `SizeLimit>=1` required for stock HARV/CMIN? -> No; default zero/absent SizeLimit goes through the JLE path and then the `Harvester=yes` gate. (evidence: `0x0073D6EC`, `0x0073D672`, `rulesmd.ini:[CMIN]/[HARV]`)
- [RESOLVED] OQ-05 - What does `RadioClass::In_Radio_Contact` return? -> True when any path array entry is nonzero, false for empty/all-zero steps. (evidence: decompile `0x0065AE30`)
- [RESOLVED] OQ-06 - What is the first PathType guard polarity? -> True jumps to RateTimer/state dispatch; false takes cleanup. (evidence: `0x0073DEE2..0x0073DEE9`)
- [RESOLVED] OQ-07 - Which branch returns `5`? -> The RateTimer-not-ready branch, not the PathType false branch. (evidence: `0x0073DF56..0x0073DFBC`)
- [RESOLVED] OQ-08 - Does the no-valid-steps branch use timer epilogue? -> No; it direct-returns `1` after optional queueing. (evidence: `0x0073DF49..0x0073DF55`)
- [RESOLVED] OQ-09 - How is the refinery found in states 3/4? -> Current cell plus hardcoded `(-1,0)`, then `Look_up_building_in_cell`. (evidence: `0x0073E181`, `0x0073E2C8`, `0x0049F2F0`, `0x0047C520`)
- [RESOLVED] OQ-10 - What triggers state 3 -> state 4? -> Threshold crossing followed by no non-empty storage slot or no positive removal; slot 8 is requested, state becomes 4, slot 10 is cleared if occupied. (evidence: `0x0073E4DC..0x0073E534`)
- [RESOLVED] OQ-11 - Does the state 3 empty transition use timer epilogue? -> No; it reaches direct `return 1`. (evidence: `0x0073E539..0x0073E5BD`)
- [RESOLVED] OQ-12 - What is `building+0x57C`? -> Slot-8 `ProductionAnim` pointer, i.e. `Anims_0[8]`. (evidence: [BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_GHIDRA_REPORT.md))
- [RESOLVED] OQ-13 - Does state 4 clear `+0x6D1` before or after the slot-8 wait? -> After the slot-8 wait guard passes. (evidence: guard `0x0073E1CB..0x0073E1EA`; clear `0x0073E1F6`)
- [RESOLVED] OQ-14 - What is the normal state-4 mission handoff? -> `SetMission(0x0A,0)`, optional radio `3`, `QueueMission`, timer epilogue. (evidence: `0x0073E24F..0x0073E2BE`)
- [RESOLVED] OQ-15 - When is radio `3` sent in normal state 4? -> Only after `SetMission(0x0A,0)` and successful vtable `+0x200`, if `RadioClass::In_Radio_Contact` returns true. (evidence: `0x0073E25A..0x0073E279`)
- [RESOLVED] OQ-16 - Is `ReleaseDockedHarvester` reachable on normal stock zero-link completion? -> No; the only call is the top nonzero-`+0x2E4` branch. (evidence: `0x0073D66D`, `0x004595C0`, writer inventory)
- [RESOLVED] OQ-17 - Is `Force_Track(0x47)` part of normal stock cargo-empty exit? -> No; it is inside `ReleaseDockedHarvester`, excluded from stock zero-link completion. (evidence: decompile `0x004595C0`, entry split `0x0073D63B`)
- [RESOLVED] OQ-18 - Are stock GAREFN/NAREFN delayed by `building+0x57C`? -> Normally no, because stock refineries do not define active slot-8 `ProductionAnim`; code path is still live and mod-sensitive. (evidence: `BUILDINGCLASS_0X57C...`; `artmd.ini:[GAREFN]/[NAREFN]`)
- [RESOLVED] OQ-19 - Does BuildingClass radio `0x15` set the refinery mission or the sender mission? -> Sender mission `0x10` for DockUnload; no reciprocal `+0x2E4` write. (evidence: decompile `0x0043C2D0`)
- [RESOLVED] OQ-20 - Does pad arrival write reciprocal `+0x2E4`? -> No; it sends radio `0x15` and locomotor/contact calls. (evidence: decompile `0x00739EC0`)
- [DEFERRED] OQ-21 - What is exact two-miner handoff timing after state-4 completion? (category: `out-of-scope`; reason: assigned as separate re-investigate/trace task; next-step-if-pursued: run the two-miner stock queue handoff investigation against BuildingClass contacts and Mission_Harvest retry timing)
- [DEFERRED] OQ-22 - How long does a modded slot-8 `ProductionAnim` keep `building+0x57C` non-null at runtime? (category: `requires-different-system-context`; reason: this report only identifies the guard and stock no-delay result; next-step-if-pursued: trace `BuildingClass::CreateAnimForSlot`, `AnimClass` lifetime, and `BuildingClass::UpdateAnimation` for slot 8)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Stock unload runs through zero `unit+0x2E4`; reciprocal release branch is not normal | `0x0073D63B`, `0x0073D66D`, writer inventory | none observed in current comments/direction | `src/sim/miner/miner_dock_sequence.rs::phase_departing`, `src/sim/miner/miner_dock.rs::RefineryDockContacts` | keep stock completion independent from reciprocal link semantics | full CMIN unload completes without `Force_Track(0x47)` or release-helper sound/effects | Do not model normal GAREFN/NAREFN completion as `ReleaseDockedHarvester` |
| `SizeLimit` absent/zero still reaches stock harvester path through `Harvester=yes` | `0x0073D6EC`, `0x0073D672`, `rulesmd.ini:[CMIN]/[HARV]` | none observed | rules/object type harvester classification and miner system entry | `Harvester=yes`, not `SizeLimit`, should classify stock miners for unload | `[CMIN]` and `[HARV]` with no `SizeLimit` still unload | Do not require `SizeLimit>=1` for refinery unload |
| `RadioClass::In_Radio_Contact` true proceeds to RateTimer/state dispatch | `0x0065AE30`, `0x0073DEE2..0x0073DEE9` | unchecked exact equivalent | future low-level mission parity if modeled | if porting this branch, preserve true/false polarity | unit with valid path steps does not take no-steps cleanup path | Avoid inverting the guard from the RED doc |
| RateTimer mismatch returns `5` directly | `0x0073DF56..0x0073DFBC` | high-level Rust pivot/timer differs | `phase_pivoting`, future mission-timer compatibility | maintain a wait result before dump-state dispatch until facing/rate window converges | miner waits/turns before first state-3 init rather than dumping while still rotating | Do not attach return `5` to PathType false |
| State 3 drains one complete StorageClass slot per dump gate | `0x0073E3BF..0x0073E457` | implemented | `phase_unloading` | keep ore-then-gem slot drain, not per-bale incremental drain | mixed ore+gem cargo credits in two pulses, ore first | Do not reintroduce per-bale credit trickle |
| State 3 empty-check occurs on a later threshold crossing and then direct-returns `1` after setting state 4 | `0x0073E4DC..0x0073E5BD` | mostly modeled by `DepositCooldown`; timing still queue-sensitive | `phase_unloading`, `phase_deposit_cooldown` | preserve one dump-gate hold after last slot drain before state-4 cleanup | full single-slot HARV does not depart immediately on the same tick as slot drain | Adjacent two-miner handoff task should verify this before changing timing |
| State 4 waits before clearing `+0x6D1` while slot-8 `ProductionAnim` pointer is non-null | `0x0073E1CB..0x0073E1F6`; `BUILDINGCLASS_0X57C...` | stock no-delay modeled; modded slot-8 wait likely missing | `phase_departing`, building anim event system | for modded refineries with `ProductionAnim`, delay state-4 cleanup until slot 8 clears | custom refinery with ProductionAnim keeps miner dock-active until anim pointer clears | Do not treat `+0x57C` as locomotor readiness |
| Stock state 4 normal exit clears `+0x6D1`, sets mission Harvest `0x0A`, optional radio `3`, queues mission, then timer epilogue | `0x0073E1F6`, `0x0073E24F..0x0073E2BE` | approximated by `phase_departing` returning to `SearchOre` | `phase_departing`, miner scheduling | stock miner should immediately re-enter harvest/search scheduling without installing release-helper exit track | after unload, miner searches ore/continues harvest loop and does not drive a special `0x47` track | Do not install cached queue-cell destination on stock zero-link exit |
| Radio `3` in normal state 4 is conditional on valid steps after `SetMission(0x0A)` | `0x0073E268..0x0073E279` | contact release is explicit Rust bookkeeping | `RefineryDockContacts`, `phase_departing` | clearing contact should remain conditional where parity requires it; current explicit release is a deterministic abstraction | no stale refinery contact after normal unload; queue can admit next miner | Queue handoff timing needs the adjacent two-miner investigation |
| `ReleaseDockedHarvester` and `Force_Track(0x47)` are conditional reciprocal-link effects | `0x004595C0`; only call `0x0073D66D` | current comments say not stock completion | `refinery_exit_cell` helper and legacy tests | keep helper only for conditional reciprocal-link/interrupt contexts | normal stock unload produces no BunkerWallsDownSound/track 0x47 | Do not reuse the helper for normal cargo-empty path |

### Stale Docs / Follow-up Docs

- Replace `MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_GHIDRA_REPORT.md` claim "stock HARV/CMIN zero-link route is `SizeLimit >= 1`" with: "stock HARV/CMIN can reach the harvester block through the `SizeLimit <= 0` path because `LAB_0073D672` tests `UnitType+0xE0E` / `Harvester=yes` and jumps to `0x0073DEE0`."
- Replace the PathType guard wording with: "`RadioClass::In_Radio_Contact != 0` jumps to `0x0073DF56` RateTimer/state dispatch; `== 0` takes cleanup, clears `+0x6D1`, optionally queues, and direct-returns `1`."
- Replace "timer epilogue is all-path convergence" with: "timer epilogue is used by state-init and normal state-4 handoff, but direct returns exist at `0x0073DFB3` (`5`) and `0x0073E5B4` (`1`), plus no-valid-steps direct `1`."
- Replace "normal stock exit is ReleaseDockedHarvester/Force_Track driven" with: "normal stock zero-link state 4 clears `+0x6D1`, calls `SetMission(0x0A,0)`, optionally radios `3`, queues mission, and reaches the timer epilogue; `ReleaseDockedHarvester` is only the nonzero-`unit+0x2E4` entry branch."
- Replace any `DAT_0089F6A0` "DockingOffset0" language with: "`DAT_0089F6A0/2` is initialized by `0x0049F2F0` as signed `(-1,0)`, a west-neighbor lookup used to rediscover the refinery."

## Sources

- Ghidra decompiled: `0x0073D630`, `0x0065AE30`, `0x004595C0`, `0x0047C520`, `0x0049F2F0`, `0x0043C2D0`, `0x00739EC0`, `0x005B35E0`, `0x005B3A00`.
- Ghidra disassembled: `0x0073D630`.
- Ghidra assembly context checked: `0x0073DEE0`, `0x0073E17F`, `0x0073E24D`, `0x0073DFB0`, `0x0073E5B1`, `0x0073E289`, `0x0073D63B`, `0x0073D6E6`, `0x0073E51C`.
- Prior docs read/reconciled:
  - [docs/research/AUDIT_LOG.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AUDIT_LOG.md)
  - `docs/research/miner/MISSION_DEPLOY_BUILDING_REFINERY_UNLOAD_GHIDRA_REPORT.md`
  - [docs/research/miner/MISSION_DEPLOY_BUILDING_DOCKED_VS_UNDOCKED_BRANCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_DEPLOY_BUILDING_DOCKED_VS_UNDOCKED_BRANCH_GHIDRA_REPORT.md)
  - [docs/research/miner/MISSION_DEPLOY_BUILDING_DAT_0089F6A0_REFINERY_LOOKUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_DEPLOY_BUILDING_DAT_0089F6A0_REFINERY_LOOKUP_GHIDRA_REPORT.md)
  - [docs/research/miner/STANDARD_REFINERY_0X2E4_WRITER_INVENTORY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/STANDARD_REFINERY_0X2E4_WRITER_INVENTORY_GHIDRA_REPORT.md)
  - `docs/research/miner/REFINERY_DOCK_ANIM_SLOTS_GHIDRA_REPORT.md`
  - [docs/research/miner/BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDINGCLASS_0X57C_DOCK_DEPART_GUARD_GHIDRA_REPORT.md)
- INI checked:
  - `ini/rulesmd.ini`
  - `ini/rules.ini`
  - `ini/artmd.ini`
- Rust scan:
  - `src/sim/miner/mod.rs`
  - `src/sim/miner/miner_dock.rs`
  - `src/sim/miner/miner_dock_sequence.rs`
