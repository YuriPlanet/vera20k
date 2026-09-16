# Radio Link State Machine for Refinery Dock — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass focused on **gaps + verification**, not on re-covering ground already documented at HIGH confidence. Execute by running `/re-investigate radio link state machine for refinery dock` with this plan loaded as context. Subagent-batch the Phase 1 inventory if needed (group sizes 5–8).

**Topic:** The full gamemd.exe `RadioClass` two-way handshake protocol **as exercised end-to-end during a refinery dock cycle** — from harvester approach (sender) through accept/queue (refinery receiver), pad arrival (TIMING_SYNC pivot), unload (Mission_Deploy_Building anim+storage loop), to normal exit (ReleaseDockedHarvester). Goal: a single unified state-machine document that names every radio case that fires during a stock-YR refinery dock, with addresses, ordering, and the actor on each side.

**Scope Size:** Medium — ~22 functions to touch (most already documented at HIGH; this pass focuses on filling case-by-case gaps and unifying the chain).

**Est. Effort:** ~6–9 hours of `/re-investigate` work (anchored: ~15–30 min per FULL function × 7 FULL + ~5–10 min × 9 MEDIUM + ~2–5 min × 6 LIGHT, plus the verification + synthesis pass).

**Prior Research:** Substantial — 20+ reports cover pieces of the chain (see Section 2). One doc is stale and needs flagging (`DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0`).

**Expected Output:**
`docs/research/RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_GHIDRA_REPORT.md`

**Next Pipeline Step:** archive-only — this is a unification + gap-closure RE doc, not a precursor to Rust work. The Rust port already implements the dock loop without modeling `RadioClass` (see Section 8). Whether to add a `RadioLink` primitive in Rust is a separate `/brainstorm` decision that the user can run after this doc lands.

---

## 1. Goal

When the executed report is done, a future session must be able to answer:

1. **What is the exact sequence of radio messages exchanged between a stock harvester (HARV/CMIN) and a stock refinery (GAREFN/NAREFN) during one full dock cycle, in tick order?**  Specifically: which case (0x07, 0x08, 0x0E, 0x10, 0x12, 0x14, 0x16, 0x17, 0x18, 0x19, …) is sent by whom, in response to what, and what state mutation happens on each side?
2. **Which radio cases visible in the four receive_radio overrides are NOT used during a normal refinery dock cycle?**  (Aircraft-only, infantry-board-transport-only, mind-control-only, TS-vestigial.)
3. **Where in the binary is the LEAVE_DOCK (0x19) and DOCKING_COMPLETE (0x07) traffic sent for refinery exit?**  Currently unverified — `ReleaseDockedHarvester` calls `Transmit_Radio_ToFirst(3)` (BREAK), but the 0x19 path is undocumented.
4. **What is the canonical Rust-friendly state-machine that reproduces the observable output (timing, ordering, side-effects) of the gamemd radio chain?**

Out of scope:
- Aircraft/helipad dock chain (different sequence; reference only).
- Slave-miner harvesting (YAREFN — does not use RadioClass at all).
- Transport/passenger boarding cases (0x0F CAN_ENTER, 0x18 dock-as-enter overload) — reference for shared receivers only.
- Repair-pad service-depot dock (different state machine, already partially documented).

## 2. Prior Research Inventory

### Protocol layer (HIGH confidence)
| Report | Scope | Conf | Gaps |
|---|---|---|---|
| `RADIO_CLASS_PROTOCOL_GHIDRA_REPORT.md` | Full RadioClass spec, ctor `0x0065A750`, Receive_Radio `0x0065A820`, Transmit_Radio_Impl `0x0065A970`, Set_Contact_Count `0x0065AE60`, Broadcast_Radio_ToAll `0x0065ACE0`, every case 0x01-0x24, RadioHistory dedup, Contacts[] vector | HIGH | None at the protocol layer; case meanings IN REFINERY DOCK CONTEXT not always traced. |
| [BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MISSILE_AND_RADIO_GHIDRA_REPORT.md) (Part 2) | BuildingClass::Receive_Radio top-level switch `0x0043C2D0`, TechnoClass::Receive_Radio `0x006F4AB0`, +0x16xx flag table | HIGH | Per-case bodies not all decompiled. |

### Refinery-dock cases (HIGH on what's covered)
| Report | Scope | Conf | Gaps |
|---|---|---|---|
| [REFINERY_RADIO_DOCKING_ACCEPTANCE_QUEUE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/REFINERY_RADIO_DOCKING_ACCEPTANCE_QUEUE_GHIDRA_REPORT.md) | HELLO(0x02) acceptance only; `BuildingClass::Receive_Radio @0x0043C2D0` slot **+0x194 (not +0x274)**, ctor `0x0043B740`, UndockUnit `0x4593A0`, Mission_Harvest send site `0x73E5E0` | HIGH | Open: unit/building `+0x2E4` layout coincidence (MEDIUM); cases 0x07/0x08/0x09 not detailed. |
| `RECEIVE_RADIO_CASE_0x0E_CAN_DOCK_GHIDRA_REPORT.md` | CAN_DOCK(0x0E) accept/reject filter chain; queue cell **hardcoded `(X+3,Y+1)` not from QueueingCell**; reply sequence MOVE_TO_CELL(0x12)→ENTER_DOCK(0x18)→TIMING_SYNC(0x16); `CanDock @0x457CE0` is NOT called from case 0x0E | HIGH | Sound cue branch (`DAT_0089c848` on 0x16 reply≠1) — player-visible cue unidentified. |
| [RADIO_0x16_SENDER_BUILDINGCLASS_CASE_0x0E_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_0x16_SENDER_BUILDINGCLASS_CASE_0x0E_GHIDRA_REPORT.md) | 0x16 sender side; resolves "TIMING_SYNC" naming vs old "FACE_DOCK" | HIGH | None. |
| [RADIO_0x16_RECEIVER_UNITCLASS_CASE_16_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RADIO_0x16_RECEIVER_UNITCLASS_CASE_16_GHIDRA_REPORT.md) | `UnitClass::Receive_Radio @0x00737430`; sets `FootClass+0x388` RateTimer to `0x4000` via `ILocomotion vtable+0x4C`; cascade sends TIMING_SYNC_BACK(0x15) | HIGH | Cases other than 0x16 in that override not decompiled. |

### Mission-state & unload side (HIGH after corrections)
| Report | Scope | Conf | Gaps |
|---|---|---|---|
| [MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MISSION_ENTER_REFINERY_DOCK_GHIDRA_REPORT.md) + `_VERIFICATION_NOTES.md` | `UnitClass::Mission_Enter` (= `PerCellProcess @0x739EC0`); state 0 vs state 2; ore overlay destroy on dock cell; piggyback CLSID_WalkLocomotion swap; dock-link write `unit+0x254` (FootClass+0x84 = DockLink); `FUN_00500200` AI wander-to-queue helper | HIGH (post-correction) | Branch identity confirmed inverted in original; helper FUN_00500200 not fully decompiled. |
| [HARVESTER_DOCK_UNLOAD_SEQUENCE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/HARVESTER_DOCK_UNLOAD_SEQUENCE.md) + `HARVESTER_DOCK_UNLOAD.md` | Lifecycle narrative; `EnterTransport @0x70FD70`, `Find_Nearest_Dock @0x004DFCB0`, `CanDock @0x457CE0` | HIGH (85%) | Stale "FACE_DOCK 0x16" claim — superseded by 0x16 docs. |
| [BUILDING_UNDOCKUNIT_0x4593A0_CHRONO_MINER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_UNDOCKUNIT_0x4593A0_CHRONO_MINER_GHIDRA_REPORT.md) | UndockUnit is **interrupt-only** (Sell/ReceiveDamage/Temporal); normal exit is ReleaseDockedHarvester; hardcoded `(-0x80, +0x80)` leptons + track 0x47 | HIGH | None. |
| [RELEASEDOCKEDHARVESTER_0x4595C0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/RELEASEDOCKEDHARVESTER_0x4595C0_GHIDRA_REPORT.md) | Normal post-dump exit; sole caller `Mission_Deploy_Building @0x0073D630`; Step 10 anchor `(NW.x-1, NW.y+1)` is vestigial; visible "east exit" is Mission_Harvest case-0 SCAN downstream | HIGH (2026-05-20 resolution) | Does 0x19 LEAVE_DOCK fire here? Unverified. |

### Multi-dock + reconciliation
| Report | Scope | Conf | Gaps |
|---|---|---|---|
| [BUILDING_DOCK_AND_HEAL_STATE_MACHINES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_DOCK_AND_HEAL_STATE_MACHINES.md) | Hospital/Armory/UnitRepair timer state machine `MissionRepairAndProduce @0x44B780`; refinery branch deferred | HIGH (hospital), NOT-COVERED (refinery dump) | Refinery dump path explicitly out of scope. |
| [BUILDING_DOCKING_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/BUILDING_DOCKING_SYSTEM_GHIDRA_REPORT.md) | BuildingTypeClass flag/offset table; `GetDockCoord @0x447B20`; per-building DockingOffset table | HIGH | "QueueingCell drives queue position" assertion misleading — case 0x0E hardcodes it. |
| [DOCK_ARRIVAL_PIVOT_SEQUENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCK_ARRIVAL_PIVOT_SEQUENCE_GHIDRA_REPORT.md) | End-to-end approach→stop→dump→exit, 4-function chain; settles 0x16 naming | HIGH | None. |
| [FIND_DOCKING_BAY_FALLBACK_ARG3_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/FIND_DOCKING_BAY_FALLBACK_ARG3_GHIDRA_REPORT.md) | `FootClass::Find_Docking_Bay @0x004DF040` is fallback only; arg3=0/1 reservation-skip semantics; `AircraftClass::FindBuildingToDock @0x0041BC17` | HIGH | None. |
| [MINER_DOCK_GAPS_RESEARCH.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/MINER_DOCK_GAPS_RESEARCH.md) | Piggyback swap-back via `FootClass::AI @0x4DA530`; building destroyed mid-unload (ore retained); AI wander-when-queued | HIGH (post 2026-05-19) | None. |
| [NUMBEROFDOCKS_VS_DOCKOFFSET_RECONCILE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/NUMBEROFDOCKS_VS_DOCKOFFSET_RECONCILE_GHIDRA_REPORT.md) | `+0x1618/+0x161C QueueingCell` (art) vs `+0x1780 NumberOfDocks` / `+0x1788 DockingOffset%d`; refinery uses only QueueingCell | HIGH | None. |

### Stale — must be verified or marked
| Report | Scope | Conf | Status |
|---|---|---|---|
| [DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0_GHIDRA_REPORT.md) | Claims `FUN_006AF6C0` is a refinery dock-queue processor | **STALE** | Scoping pass confirms `0x006AF6C0 = SlaveManagerClass::AI_Update`. There is NO standalone DockManager class. **The investigation must re-verify and the executor must either replace the doc or move it to a deprecated/ folder.** |

### In-repo plans
- `docs/plans/2026-05-06-refinery-dock-gamemd-parity-design.md` + `-plan.md` — 4-phase Rust rewrite around `Mission_Deploy_Building @0x73D630`. Implementation plan; not RE.

### Traces (fidelity slot-by-slot, all 2026-05-19)
- [CHRONO_MINER_REFINERY_UNDOCK_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/traces/CHRONO_MINER_REFINERY_UNDOCK_TRACE.md) (22 stages U01–U22)
- [CHRONO_MINER_TELEPORT_DOCK_APPROACH_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/traces/CHRONO_MINER_TELEPORT_DOCK_APPROACH_TRACE.md)
- [CHRONO_MINER_ORE_DUMP_DEPOSIT_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/traces/CHRONO_MINER_ORE_DUMP_DEPOSIT_TRACE.md) (20 stages)
- [CHRONO_MINER_MISSION_HARVEST_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/traces/CHRONO_MINER_MISSION_HARVEST_TRACE.md), `_LOCOMOTION_DRIVE_PHASE_TRACE.md`, `_FORCE_TRACK_0X47_BIB_STEP_TRACE.md`, `_POST_DUMP_EXIT_WALK_RETRACE.md`, `_PAD_PIVOT_TO_EAST_TRACE.md`, `_TELEPORT_INBOUND_VISUAL_CHAIN_TRACE.md`, `_TOO_FAR_THRESHOLD_BRANCH_TRACE.md`

**Conflicts between reports:**
- "FACE_DOCK 0x16" (old HARVESTER_DOCK_UNLOAD) ↔ "TIMING_SYNC 0x16" (RADIO_0x16_*) — new wins.
- Mission_Enter §5.3 harvester/non-harvester branches inverted in original — corrected in VERIFICATION_NOTES.
- `FUN_006AF6C0` mislabeled as DockManager — actually `SlaveManagerClass::AI_Update`.
- `UndockUnit` as "normal exit" — wrong; interrupt-only.
- `BuildingClass::Receive_Radio` at vtable+0x274 — wrong; correct slot is +0x194.
- QueueingCell drives queue position — misleading; case 0x0E hardcodes `(X+3,Y+1)`.

## 3. Function Inventory

| # | Phase | Address | Current Name (Ghidra) | Scope Reason | Depth Target | TS-Legacy Risk |
|---|---|---|---|---|---|---|
| 1 | 1 | `0x0065A820` | `RadioClass::Receive_Radio` | Base virtual; cases 0x01-0x04, 0x0A, 0x0C protocol primitives | **FULL** | Low |
| 2 | 1 | `0x0065A970` | `RadioClass::Transmit_Radio_Impl` | Core sender; filters via `Filter_AbstractType_InMap`, invokes target vtable+0x194 | **FULL** | Low |
| 3 | 1 | `0x0065AAA0` | `RadioClass::Transmit_Radio` | Arg-marshalling wrapper around Impl | **MEDIUM** | Low |
| 4 | 1 | `0x0065ACB0` | `RadioClass::Transmit_Radio_ToFirst` | Unicast variant for 0x16/0x18 chain | **MEDIUM** | Low |
| 5 | 1 | `0x0065AD90` | `RadioClass::FindDockSlot` | 4-iter loop over contacts; returns slot index | **LIGHT** | Low |
| 6 | 1 | `0x0065ADF0` | `FUN_0065adf0` (FindFreeContactSlot) | Free-slot probe; called from EnterBuildingOrDock + BuildingClass::Receive_Radio | **MEDIUM** | Low — sibling of #5 |
| 7 | 1 | `0x0043C2D0` | `BuildingClass::Receive_Radio` | **REFINERY SIDE** — 999 instr, 93 CC, cases 0x07/08/09/0A/0C/0E/0F/10/12/13/14/16/17/18/22/23. CAN_DOCK body + queue admission live here. | **FULL** | Medium — cases 0x12, 0x13, 0x14, 0x22, 0x23 may be TS spawner/AI-deploy; verify per case |
| 8 | 1 | `0x00737430` | `UnitClass::Receive_Radio` | **HARVESTER SIDE** — 606 instr, 49 CC, cases 0x06/09/0A/0C/0E/10/12/14/16/18/21. 0x16 RateTimer pivot already covered. | **FULL** | Low — case 0x21 (33) is novel, verify it isn't TS deploy |
| 9 | 1 | `0x0073D630` | `UnitClass::Mission_Deploy_Building` | **UNLOAD CYCLE OWNER** — 1177 instr, 107 CC. Calls ReleaseDockedHarvester + SetAnimSlotImage + Add_Tiberium_To_Storage + Add_Tiberium_Credits. Single caller of ReleaseDockedHarvester. | **FULL** | Low |
| 10 | 2 | `0x0041AA80` | `UnitClass::EnterBuildingOrDock` | **SENDER SIDE OF APPROACH** — 274 instr, 37 CC. Calls FUN_0065adf0 + Set_Destination_Internal + Filter_AbstractType_InMap. This is where 0x0E/0x16 traffic originates. **Currently UNDOCUMENTED.** | **FULL** | Low |
| 11 | 2 | `0x004D8FB0` | `FootClass::Receive_Radio` | 236 instr, cases 0x05/07/08/09/0A/0C/12/14/18/20/C8. 0x16 routes here from UnitClass via "NEED_TO_MOVE got ROGER" branch (per existing doc). | **FULL** | Low — case 0xC8 (200) is suspicious, flag for TS check |
| 12 | 2 | `0x006F4AB0` | `TechnoClass::Receive_Radio` | Generic Techno fallthrough; 309 instr, cases 25-50, 0x02/03/0A/0C. Calls WarpAttachClass::Detach, Spend_Money. | **FULL** | Medium — case 50 (0x32) and 44 (0x2C) flagged for YR-live verification |
| 13 | 2 | `0x004D9290` | `FootClass::Mission_Enter` | 166 instr — dispatch shell. Calls FUN_0045af20 (likely thunk to EnterBuildingOrDock). | **FULL** | Low — verify FUN_0045af20 resolves correctly |
| 14 | 2 | `0x004595C0` | `BuildingClass::ReleaseDockedHarvester` | Normal post-dump exit. **OPEN QUESTION: does it emit 0x19 LEAVE_DOCK in addition to BREAK(0x03)?** | **FULL** | Low |
| 15 | 2 | `0x004593A0` | `BuildingClass::UndockUnit` | Interrupt-only. Re-verify radio behavior (Sell/ReceiveDamage/Temporal paths). | **MEDIUM** | Low |
| 16 | 2 | `0x0044EFB0` | `BuildingClass::GetDockCellForObject` | 494 instr, 32 CC. Source of case-0x0E coordinate transmitted back to harvester. | **FULL** | Medium — large; verify which branches are YR-live for refinery |
| 17 | 2 | `0x00451890` | `BuildingClass::CreateAnimForSlot` | The refinery dock-anim swap site; called by case 0x0E, ReleaseDockedHarvester, Mission_Deploy_Building | **MEDIUM** | Low |
| 18 | 2 | `0x00451750` | `BuildingClass::SetAnimSlotImage` | Frame swap during unload (Anim1→2→3) | **MEDIUM** | Low |
| 19 | 2 | `0x00451E40` | `BuildingClass::ClearAnimSlot` | Slot terminator | **LIGHT** | Low |
| 20 | 3 | `0x004DFCB0` | `FootClass::Find_Nearest_Dock` | Harvester selects refinery before linking | **MEDIUM** | Low |
| 21 | 3 | `0x00447B20` | `BuildingClass::GetDockCoord` | Paired with #16 | **LIGHT** | Low |
| 22 | 3 | `0x004190B0` | `AircraftClass::Receive_Radio` | Reference only — aircraft cases overlap radio IDs; document overlap | **LIGHT** | Medium — many TS spyplane/aircraft cases |
| 23 | 3 | `0x005F5320` | `ObjectClass::Receive_Radio` | Terminal swallow for unhandled cases — confirms case-not-handled fallthrough | **LIGHT** | Low |
| 24 | 3 | `0x00500200` | `FUN_00500200` (AI wander-to-queue) | Cited by MISSION_ENTER_VERIFICATION_NOTES; never fully decompiled | **MEDIUM** | Medium — verify it fires for player-controlled harvesters too |
| 25 | 3 | `0x0073E5E0` | `UnitClass::Mission_Harvest` | Outer harvest loop; reference for return-path linkage to Mission_Enter | **LIGHT** | Low |
| 26 | 3 | `0x00740EF0` | `UnitClass::Mission_Unload` | 49 instr — thin timer entry, likely TS-vestigial in YR refinery path | **LIGHT** | **High** — confirm reachability in YR; may be dead |
| 27 | 3 | `0x006AF6C0` | `SlaveManagerClass::AI_Update` (NOT DockManager) | **Negative-finding verification** — confirm it has nothing to do with refinery dock | **LIGHT** | n/a — verification only |

**Total: 27 functions (7 FULL, 9 MEDIUM, 11 LIGHT, 1 verification-only).**

**Phase 1 checkpoint:** after #1–#9 are decompiled, the executor must publish a partial state-machine diagram and pause for review before starting Phase 2. If the diagram shows the case list is incomplete or the chain has unexpected branches (e.g., a TS-only mid-unload case fires), the plan is revised.

## 4. Detail Checklist

### Radio cases to fully enumerate (in refinery dock context)

For each case ID below, the report must answer: who sends it, who receives it, in what tick of the dock cycle, with what payload, producing what side effect on each side. Categorize as: ACTIVE-IN-REFINERY-DOCK / SHARED-ELSEWHERE-NOT-REFINERY / TS-VESTIGIAL / UNUSED-IN-YR.

| Case | Status from prior research | Needed in this pass |
|---|---|---|
| 0x01 ROGER | Reply value (protocol doc) | Confirm it's the reply to every accepted message in the dock chain |
| 0x02 HELLO | HIGH — REFINERY_RADIO_DOCKING_ACCEPTANCE | Confirm when (if ever) HELLO fires during a refinery dock vs. only the 0x0E path |
| 0x03 BREAK / OVER_AND_OUT | Protocol doc | Confirm ReleaseDockedHarvester's `Transmit_Radio_ToFirst(3)` is the exit BREAK |
| 0x07 DOCKING_COMPLETE | Table-only | **Open** — does refinery emit 0x07 after the last bale? |
| 0x08 REQUEST_DOCKING_CLEARANCE | Table + MISSION_ENTER_VERIFICATION (returns 0x17 QUEUED) | Trace the full path in BuildingClass::Receive_Radio case 0x08 |
| 0x09 | Mirror of 0x07 | Confirm role in refinery dock |
| 0x0A NEGATORY | Reply | Confirm when it's the reply (likely failed CanDock) |
| 0x0B DOCK_APPROACH | Table-only — "building→unit Queue_Mission(UNLOAD=0x14)" | **Open** — does refinery actually send 0x0B during the dock chain, or is this TS-only? |
| 0x0C DOCK_ARRIVED | Table-only | **Open** — does harvester send 0x0C inbound vs. relying on Mission_Enter PerCellProcess detection? |
| 0x0D ambient-anim reset | Protocol doc | Confirm no role |
| 0x0E CAN_DOCK | HIGH | Already documented; confirm reply chain still matches |
| 0x0F CAN_ENTER | Transport-only doc | Confirm not used in refinery dock |
| 0x10 RESERVE_DOCK / PrepareToLink | Protocol doc | **Open** — does refinery dock use 0x10 during approach, or is it aircraft/helipad only? |
| 0x11 IS_UNIT_LINKED | Inferred only | **Open** — when does the harvester poll this during dock? |
| 0x12 MOVE_TO_CELL | HIGH (case 0x0E chain) | Already covered |
| 0x13 NEED_TO_MOVE | HIGH (case 0x0E chain) | Already covered |
| 0x14 CELL_ACCEPTED | HIGH | Already covered |
| 0x15 DOCK_NOW / TIMING_SYNC_BACK | HIGH (0x16 cascade) | Already covered |
| 0x16 TIMING_SYNC | HIGH (sender + receiver) | Already covered |
| 0x17 QUEUED + EVICT_QUEUE | HIGH (via Mission_Enter docs) | Confirm eviction triggers |
| 0x18 ENTER_DOCK | HIGH | Already covered |
| 0x19 LEAVE_DOCK | Table-only — **NOT TRACED FOR REFINERY EXIT** | **PRIMARY OPEN QUESTION** — find if/when it fires during ReleaseDockedHarvester or Mission_Deploy_Building exit |
| 0x1A / 0x1B secondary dock lock | Table-only | Confirm not used in refinery |
| 0x1C–0x24 | Non-refinery | Out of scope |

### Magic numbers and constants to extract

- `BuildingClass::Receive_Radio` immediate values: `4800, 4748, 4732, 4816, 128, 71` (from ReleaseDockedHarvester) and the case-0x0E queue hardcode `(NW.x + 3, NW.y + 1)` cells
- `UnitClass::Receive_Radio` immediate: `0x4000` RateTimer for 0x16 pivot
- `UndockUnit` immediates: `128, 1, 3, 71, 12` and the `(-0x80, +0x80)` leptons + track `0x47`
- `FootClass::Receive_Radio` cases including suspicious `0xC8 (200)` — verify it's not TS
- Anim slot indices in CreateAnimForSlot (which slot is dock-unload anim — see existing REFINERY_DOCK_ANIM_SLOTS doc)
- Contact array size (currently believed 4) — confirm in RadioClass ctor
- Radio dedup history length — confirm

### Bit flags

- `+0x16xx` flag table on BuildingTypeClass (already mapped) — confirm what filters CAN_DOCK uses
- `AbstractFlags & 1` bidirectional ally check (HELLO accept condition 3) — **never explained for which entity types it fires**; resolve in this pass

### State machine states

- Harvester radio-driven state across one dock cycle. Build a single Mermaid/ASCII diagram from these owners:
  - `UnitClass::EnterBuildingOrDock @0x0041AA80` (sender)
  - `UnitClass::Receive_Radio @0x00737430` (receiver)
  - `FootClass::Mission_Enter @0x004D9290` (mission dispatch)
  - `UnitClass::Mission_Deploy_Building @0x0073D630` (unload cycle)
- Refinery radio-driven state:
  - `BuildingClass::Receive_Radio @0x0043C2D0` (receiver)
  - `BuildingClass::ReleaseDockedHarvester @0x004595C0` (exit)
  - `BuildingClass::UndockUnit @0x004593A0` (interrupt)

### INI keys to verify the binary reads (cross-check with Agent B inventory)

- `NumberOfDocks` — verify case 0x0E gates on this even when refinery NumberOfDocks=1
- `Refinery=`, `DockUnload=` — verify exactly which switch case checks them
- `Dock=` (UnitType) — verify Find_Nearest_Dock filters on this
- `QueueingCell=` (art.ini) — confirm prior finding that case 0x0E ignores it
- `DockingOffset0=` — confirm Receive_Radio path does/doesn't read it
- TS-legacy `WaitingOffset0..7` + `revertNumberOfWaitingPoints=8` — verify binary still parses (probably dead)

### Struct offsets to extract (note `param_1` type)

- BuildingClass `+0x16xx` flag block — already mostly mapped
- FootClass `+0x84` = DockLink (already documented as `unit+0x254` write target)
- FootClass `+0x388` = RateTimer (set during 0x16)
- BuildingClass `+0x2E4` = unit/building radio-coincidence offset (open question — confirm or deny)
- BuildingClass Contacts[] — confirm slot count + size

### Edge cases to verify

- Harvester arrives when refinery dock is full → 0x17 QUEUED → wander loop (`FUN_00500200`)
- Higher-priority harvester arrives at full dock → does eviction fire? (RadioClass::Transmit_Radio_Impl §3 says HELLO evicts Contacts[0] via BREAK; verify for refinery)
- Refinery sold mid-dock → UndockUnit interrupt path
- Refinery destroyed mid-dock → ore retained on miner (per MINER_DOCK_GAPS); verify radio cleanup
- Temporal weapon on docked harvester → UndockUnit
- Chrono miner inbound warp landing → verify `BuildingClass::DeployUnit_ChronoWarp @0x0070FEE0` doesn't bypass radio handshake

### Timing / ordering

- Where does refinery dock state advance relative to `World::advance_tick`? Document gamemd's tick-pipeline position of each function (Mission_AI vs Building_AI vs Logic update).
- Per-tick frequency of each case in a normal dock (case 0x16 fires once, 0x18 fires once, etc.)

### TS-legacy flags (consolidated in §7)

- `SpecialFlags & 0x1000` — any gated functions in this set?
- `0x21 (33)` case in UnitClass::Receive_Radio — could be TS Deploy
- `0xC8 (200)` case in FootClass::Receive_Radio — definitely TS-flavored
- `0x32 (50)` and `0x2C (44)` in TechnoClass::Receive_Radio
- `0xF7, 0xFC` (negative) branches in AircraftClass::Receive_Radio
- `UnitClass::Mission_Unload @0x00740EF0` (49 instr) — likely TS-vestigial in YR

### Vtable dispatches to resolve

- Every Receive_Radio override is dispatched via vtable+0x194 (confirmed). Confirm Transmit_Message uses the same slot on its targets. Confirm BuildingClass::Receive_Radio's slot was previously docs-claimed at +0x274 — re-verify +0x194 by reading vtable bytes (CLAUDE.md `feedback_vtable_binding_verification`).
- `ILocomotion vtable+0x4C` (called from 0x16 receiver to set RateTimer) — confirm method identity by reading TeleportLocomotion / DriveLocomotion vtables.

## 5. INI Keys in Scope

Imported from Agent B; only the entries that touch the radio chain are listed here. The full INI inventory lives in the scoping report.

| Key | Section | Default | Suspected Purpose | Currently parsed in Rust? |
|---|---|---|---|---|
| `Refinery=` | BuildingType | `yes` GAREFN/NAREFN; absent on YAREFN | Marks valid radio-dock target | **Partial** — `Dock=` harvester-side list is read; refinery `Refinery=` flag handling not consolidated |
| `DockUnload=` | BuildingType | `yes` GAREFN/NAREFN | Triggers unload state after Move_to_me arrival | Yes (effectively — `dock_phase` runs unconditionally for refinery) |
| `NumberOfDocks=` | BuildingType | `1` (refineries); `4` GAAIRC/NAWEAP | Max linked harvesters; gates Receive_Radio | **No for refineries** (single-slot only); Yes for AirfieldDocks |
| `Storage=` | BuildingType | `200` (refineries) | Bail capacity added to player credits | Yes (bale_events → storage) |
| `NumberImpassableRows=` | BuildingType | `3` GAREFN/NAREFN | Blocks RadioContact drive-through cells | **Unknown — verify in `src/sim/`** |
| `ResourceDestination=` | BuildingType | `yes` | AI dock-target flag | Partial (AI not in scope yet) |
| `FreeUnit=` | BuildingType | `CMIN` GAREFN, `HARV` NAREFN | Free post-buildup harvester | Yes |
| `Bib=` | BuildingType | `yes` | Adds the dock-driveable bib row | Yes |
| `revertNumberOfWaitingPoints=` | BuildingType | commented out (TS legacy) | Old waiting queue | No — TS legacy, leave |
| `WaitingOffset0..7=` | BuildingType (art) | all commented | TS waiting-ring slots | No — TS legacy |
| `Dock=` | UnitType | `NAREFN,GAREFN` HARV/CMIN | Allowed refineries | Yes |
| `Harvester=` | UnitType | `yes` | Activates harvest/dock cycle | Yes |
| `Storage=` | UnitType | `40` HARV / `20` CMIN | Bail capacity | Yes |
| `UnloadingClass=` | UnitType | `HORV` HARV / `CMON` CMIN | Visual swap during dock | Yes (`display_type_override`) |
| `Teleporter=` | UnitType | `yes` (CMIN only) | Chrono inbound locomotor swap | Yes |
| `QueueingCell=` | BuildingType (art) | `4,1` GAREFN/NAREFN | **Stale — case 0x0E hardcodes** | Yes (used in `refinery_queue_cell`) |
| `DockingOffset0=` | BuildingType (art) | declared/commented per refinery | Pad offset; case 0x0E may or may not use | Yes (used in `refinery_pad_cell` fallback) |
| `ActiveAnim..ActiveAnimFour=` | BuildingType (art) | NAREFNL1..L4 / GAREFNL1..L4 | Dock-unload anim slots | Yes (BuildingAnimKind::Special) |
| `SpecialAnim=` | BuildingType (art) | NAREFNOR / GAREFNOR | Dock-unload SHP | Yes |
| `HarvestersPerRefinery=` | [General] | `2,2,1` | AI ratio | No (AI deferred) |
| `HarvesterTooFarDistance=` | [General] | `5` cells | Drive-instead-of-reserve threshold | Yes (`HarvesterTooFarDistance` in MinerConfig) |
| `HarvesterUnit=` | [General] | `HARV,CMIN` | AI buildable harvesters | No (AI deferred) |

## 6. Caller & Integration Map

### Gamemd-side callers (from Agent D)

| Caller | Calls Into | When | Decompile? |
|---|---|---|---|
| `UnitClass::Mission_Enter` vtable | `FootClass::Mission_Enter @0x004D9290` → `FUN_0045af20` → `UnitClass::EnterBuildingOrDock @0x0041AA80` | Each tick the harvester is in MISSION_ENTER | **YES** — full chain (Phase 2 #10, #13) |
| `UnitClass::Mission_Harvest @0x0073E5E0` | Dispatches back into Mission_Enter on full-cargo | Tick loop | **LIGHT** — confirm dispatch site only |
| `UnitClass::Mission_Deploy_Building @0x0073D630` | `BuildingClass::ReleaseDockedHarvester @0x004595C0` | After last bale, single caller | **YES** (#9 + #14) |
| `BuildingClass::Sell @0x0044AAB0`, `BuildingClass::ReceiveDamage @0x004424EA`, `TemporalClass::Update @0x0071AA15` | `BuildingClass::UndockUnit @0x004593A0` | Interrupt paths | **LIGHT** — confirm those are still the only 3 callers in YR |
| Vtable-only entries | Every Receive_Radio override | Each Transmit_Radio_Impl call | n/a (vtable dispatch is the path) |

Callers explicitly NOT investigated:
- `AircraftClass::FindBuildingToDock` (airfield only)
- `BuildingClass::CanAutoDeployHere` (AI deploy logic, not refinery dock)
- `GrandOpening` (post-construction; only adjacent)

### Rust integration map

Where the executed report's findings will hook in (informational — this plan does not implement):

- Current Rust dock loop lives in `src/sim/miner/miner_dock_sequence.rs` (785 lines, 6-phase FSM). No `RadioClass` analog.
- Refinery-side reservations: `src/sim/miner/miner_dock.rs:DockReservations` (single-slot BTreeMap; does NOT model NumberOfDocks).
- Building-anim coupling: `src/app_building_anim.rs:338–470` consumes `BaleDepositEvent` and triggers slot anims.
- Tick integration: `World::advance_tick` calls `tick_building_docks` and `tick_aircraft_docks` in Phase 7 (`src/sim/world/mod.rs:1447–1448`); `tick_miners` dispatched separately.
- Tests in `src/sim/miner/miner_tests.rs` — 67 tests, many dock-specific.

If post-report the user decides to introduce a `RadioLink` primitive, the natural seams are: (a) replace `DockReservations` with a generic `RadioContacts` allocator, (b) lift `AirfieldDocks` multi-pad pattern over refineries, (c) move dock phase transitions into a generic radio-message-driven state machine.

## 7. TS-Legacy Risk Register

Consolidated traps the executor must verify before reporting any of these as live YR behavior:

- **`UnitClass::Mission_Unload @0x00740EF0`** (49 instr) — likely TS-vestigial in a YR refinery dock. Trace callers; if no YR-reachable path, document as dead.
- **`UnitClass::Receive_Radio` case 0x21 (33)** — novel ID, possibly TS Deploy. Confirm reachability.
- **`FootClass::Receive_Radio` case 0xC8 (200)** — abnormally high ID; flag for TS-only verification.
- **`TechnoClass::Receive_Radio` cases 0x32 (50), 0x2C (44)** — likely capture/mind-control/warp; verify each is live in YR.
- **`AircraftClass::Receive_Radio` cases 0xF7, 0xFC** — negative-int branches; likely error returns or TS spyplane code.
- **`revertNumberOfWaitingPoints=` and `WaitingOffset0..7=`** — INI keys commented out in stock; verify gamemd no longer reads them.
- **`SpecialFlags & 0x1000`** — global TS gate; cross-reference any case body that branches on it.
- **`BuildingClass::Receive_Radio` cases 0x22 (34), 0x23 (35)** — undocumented; check if AI-spawner/grand-opening era code.
- **`MissionRepairAndProduce @0x44B780`** "dock/undock with IPiggyback locomotion" fall-through — already noted as possibly-dead for refineries; confirm in this pass.

For every case the report claims is live, state "Active in YR: Yes/No/Conditional (on flag X)" with the verification call cited (CLAUDE.md "Cite the verification call inline" rule).

## 8. Current Rust Implementation Surface

From Agent C, listed for executor cross-reference (no action expected during this RE pass):

- `src/sim/miner/mod.rs` — `MinerKind`, `MinerState` (8 variants), `RefineryDockPhase` (6 phases), `Miner` struct, `MinerConfig::from_general_rules`
- `src/sim/miner/miner_system.rs` (1323 lines) — `tick_miners`, `process_miner`, `handle_return`, `find_nearest_refinery`
- `src/sim/miner/miner_dock_sequence.rs` (785 lines) — full dock sub-FSM, the 6 phase handlers, deferred TODO at line 676 (`force_track_0x47_bib_step`)
- `src/sim/miner/miner_dock.rs` (175 lines) — `DockReservations` (single-slot BTreeMap + per-refinery VecDeque queue)
- `src/sim/docking/building_dock.rs` (358 lines) — `DockState`/`DockPhase` for REPAIR depots (not refineries)
- `src/sim/docking/aircraft_dock.rs` (786 lines) — `AirfieldDocks` (does honor NumberOfDocks; pattern to lift)
- `src/sim/docking/pad_geometry.rs` (115 lines) — shared lepton→cell pad math
- `src/app_building_anim.rs:338–470` — bale_events → anim slot + RefinerySmokeOffsets particles
- `src/sim/world/mod.rs:1447–1448` — tick integration
- `src/sim/world/world_hash.rs:173–177, 430–431` — determinism hash coverage
- Tests: `src/sim/miner/miner_tests.rs` (67 tests, dock-specific listed in scoping report)
- No `RadioClass` analog. No `RadioLink` type. Comments at `miner_dock_sequence.rs:42` and `mod.rs:95` cite the gamemd radio addresses for context.

## 9. Deferred Open Questions

Carry these into the executor's must-resolve list:

1. **Does 0x19 LEAVE_DOCK fire on refinery exit, in addition to BREAK(0x03)?** Read `ReleaseDockedHarvester @0x004595C0` and `Mission_Deploy_Building @0x0073D630` end-of-cycle.
2. **Does 0x07 DOCKING_COMPLETE fire after the last bale deposit?** Same scope.
3. **Does the refinery emit 0x0B DOCK_APPROACH when the harvester is en route, or is that TS-only / aircraft-only?** Read `BuildingClass::Receive_Radio` case 0x08 body.
4. **Does the harvester send 0x0C DOCK_ARRIVED inbound, or rely on Mission_Enter PerCellProcess to detect arrival?** Read `Mission_Enter @0x004D9290` body.
5. **What is the role of 0x10 RESERVE_DOCK / 0x11 IS_UNIT_LINKED in the refinery chain (vs. aircraft)?**
6. **Does Contacts[0] eviction (HELLO with full slots → BREAK old contact, per Transmit_Radio_Impl §3) trigger at a refinery?** Test a contrived eviction scenario in the doc walk-through.
7. **What is `BuildingClass+0x2E4`** (the unit/building radio-coincidence offset open question from REFINERY_RADIO_DOCKING_ACCEPTANCE)?
8. **Is `FUN_00500200` (AI wander-to-queue) reachable from player-controlled harvesters, or AI only?**
9. **Is `Mission_Unload @0x00740EF0` reachable in YR, or fully dead?**
10. **What sound cue does case 0x0E play when the 0x16 reply ≠ 1?** (Open from RECEIVE_RADIO_CASE_0x0E.)
11. **Is the case-0x0E queue-cell hardcode `(NW.x+3, NW.y+1)`** width-dependent? Test a hypothetical 6-wide refinery (mod hypothetical).
12. **Re-verify [DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0_GHIDRA_REPORT.md)** — confirm `0x006AF6C0` is `SlaveManagerClass::AI_Update` and not a dock manager; flag the doc to be deprecated.
13. **Re-verify `BuildingClass::Receive_Radio` is dispatched via vtable+0x194 (not +0x274)** by reading the BuildingClass vtable bytes directly (vtable-binding verification per CLAUDE.md feedback memory).

## 10. Execution Strategy

**Recommended: Batched subagents, two phases with checkpoint.**

- **Phase 1 (Core, 7 FULL + 2 supporting)**: dispatch in one batch of 5 subagents.
  - Agent 1: `RadioClass::*` — items #1, #2, #3, #4
  - Agent 2: `BuildingClass::Receive_Radio @0x0043C2D0` — item #7 (full case-by-case)
  - Agent 3: `UnitClass::Receive_Radio @0x00737430` — item #8 (full case-by-case)
  - Agent 4: `Mission_Deploy_Building @0x0073D630` — item #9
  - Agent 5: vtable verification + slot probes — items #5, #6, #23 + the BuildingClass vtable+0x194 binding check + open question #13

  **Checkpoint**: synthesize a draft state-machine diagram from these 5 reports. Verify all case IDs map cleanly. If anything contradicts the prior HIGH-confidence docs, pause and reconcile before Phase 2.

- **Phase 2 (Depth, 5 FULL + 6 MEDIUM)**: second batch of 5 subagents.
  - Agent 6: `EnterBuildingOrDock @0x0041AA80` — item #10 (sender side, undocumented)
  - Agent 7: `FootClass::Receive_Radio @0x004D8FB0` + `TechnoClass::Receive_Radio @0x006F4AB0` — items #11, #12
  - Agent 8: `FootClass::Mission_Enter @0x004D9290` + `ReleaseDockedHarvester @0x004595C0` + `UndockUnit @0x004593A0` — items #13, #14, #15 + **answer open questions 1, 2**
  - Agent 9: `GetDockCellForObject @0x0044EFB0` + `CreateAnimForSlot @0x00451890` + `SetAnimSlotImage @0x00451750` + `ClearAnimSlot @0x00451E40` — items #16, #17, #18, #19
  - Agent 10: TS-legacy sweep — items #20, #21, #22, #24, #25, #26, #27 (each LIGHT; one agent can cover them all)

- **Phase 3 (Synthesis)**: single executor pass merges the 10 agent reports into [RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/RADIO_LINK_REFINERY_DOCK_STATE_MACHINE_GHIDRA_REPORT.md), resolves all 13 open questions, and ships the deprecation note for the stale DOCKMANAGER doc.

Single-session `/re-investigate` is also workable but slower; given the scope and the heavy use of prior docs, batched is more efficient.

## 11. Success Criteria

The executed research document must:

- Answer every question in Section 1 (the four top-level goals).
- Cover every function in Section 3 (or explicitly justify omission with a one-line reason).
- Resolve every open question in Section 9 — either with a verified answer or with explicit "UNVERIFIABLE in this session, escalate to next pass" status (per CLAUDE.md "no invented facts").
- For every function decompiled, state **"Active in YR: Yes / No / Conditional (on flag X)"** with the verification call cited inline.
- Cite Ghidra MCP calls inline for every address, offset, vtable slot, and case-body claim (CLAUDE.md "Cite the verification call inline" rule).
- Include a single state-machine diagram (Mermaid or ASCII) showing both the harvester-side and refinery-side state across one full dock cycle, with every radio case labeled in tick order.
- Include a deprecation note for [DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/DOCKMANAGER_STATE_MACHINE_FUN_006AF6C0_GHIDRA_REPORT.md) (recommend moving to `deprecated/` or rewriting as `SLAVE_MANAGER_AI_UPDATE_GHIDRA_REPORT.md`).
- Cross-link to all prior HIGH-confidence reports in Section 2 rather than re-deriving their findings.
- Confidence label per claim (HIGH / MEDIUM / LOW) with the 3 axes from `feedback_research_confidence_axes`: content, identity, binding.

## Sources

**Ghidra addresses sampled during scoping (confirmed live):** 0x0065A750, 0x0065A820, 0x0065A970, 0x0065AAA0, 0x0065ACB0, 0x0065ACE0, 0x0065AD90, 0x0065ADF0, 0x0065AE60, 0x005F5320, 0x006F4AB0, 0x004D8FB0, 0x0043C2D0, 0x00737430, 0x004190B0, 0x004595C0, 0x004593A0, 0x0041AA80, 0x004D9290, 0x0073D630, 0x0073E5E0, 0x00740EF0, 0x00451890, 0x00451750, 0x00451E40, 0x00457CE0, 0x0044EFB0, 0x00447B20, 0x004DFCB0, 0x004DF040, 0x006AF6C0 (verified as SlaveManagerClass::AI_Update — NOT DockManager).

**Docs searched:** `docs/research/` (all RADIO_*, DOCK_*, HARVEST*, REFINERY_*, MINER_*, MISSION_ENTER*, MISSION_DEPLOY*, RELEASEDOCKED*, UNDOCK*, FIND_DOCKING*, BUILDINGCLASS_MISSILE_AND_RADIO, BUILDING_DOCKING_SYSTEM*); `docs/plans/2026-05-06-refinery-dock-gamemd-parity-*`; the `traces/` subdir.

**INI files checked:** `ini/rulesmd.ini`, `ini/artmd.ini`, `ini/rules.ini`, `ini/art.ini`.

**Related plans:** `docs/plans/2026-05-06-refinery-dock-gamemd-parity-design.md` and `-plan.md` (implementation, not RE).
