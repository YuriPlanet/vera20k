# Ground Movement Atomic-Flip Readiness Investigation Plan

**Date:** 2026-07-20

**Status:** IN PROGRESS — Checkpoint A complete and cold-reviewed PASS; Checkpoints B–E remain pending and production activation remains blocked.

**Design:** `docs/plans/2026-07-20-per-object-ground-movement-drive-process-design.md`

**Implementation contract:** `docs/contracts/2026-07-20-ground-drive-process-track-stepping-implementation-contract.md`

## Goal

Close only the evidence gaps that prevent a parity-safe, atomic migration of the complete current Phase-1 ground population from Rust's global `tick_movement_with_grids` owner into the existing live per-object host.

The first checkpoint corrected the stale Foot Mission_Move research and established the exact native `TechnoClass::AI_Update -> Mission_Dispatch -> FootClass::AI gates -> locomotor Process -> Foot return` contract. Checkpoint A is complete and its separately reviewed implementation plan now authorizes only the approved inert/test-only ordinary-Drive harness.

This plan produces research and corrected documentation only. It does not write Rust, run Cargo, change production authority, stage, commit, or mutate the Ghidra program.

## Binding Architecture Decision

The approved production decision is not being reopened:

- the final behavior-bearing flip is atomic across ordinary Unit/Drive, miner, Infantry/Walk, Hover, Ship, forced Drive tracks, and active low-bridge tubes, together with their gameplay-bearing lifecycle/effect owners;
- ordinary Drive may be researched, mechanically extracted, or exercised read-only/on cloned fixtures first;
- a live vehicle-only production flip or a production `handled_vehicle_ids` skip bridge is known DRIFT and remains forbidden;
- at the eventual flip, every prepared Phase-1 ground handler activates in the live object slot and the entire standalone production `tick_movement_with_grids` call retires in the same change.

## Why More Research Is Required

Static scheduling is resolved. [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md) proves one main live-object pass per reached Main_Tick, one locomotor Process opportunity per eligible Foot turn, no separate 15 Hz Drive movement gate, and a late `g_CurrentFrameCounter` increment.

Checkpoint A closed the first two evidence gaps below; their current-Rust disparities remain real but no longer block the inert harness. The production blockers are now Checkpoints B–E:

1. **Evidence CLOSED; Rust DRIFT remains.** Current Rust does not implement the now-verified segmented Techno/Mission/Foot host. It uses an empty `techno_common_pre`, an alive check at the wrong semantic point, a `derived_mission` projection instead of actual Mission_Dispatch/Mission_Move timer behavior, a damage-Spark-only `techno_common_post`, no guard E, and no authoritative Foot gate bracket.
2. **CLOSED.** [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) was corrected for Scenario RNG ownership, API-call/raw-draw distinction, timer-count language, Unit binding, Infantry width/slot semantics, and the `Is_Moving` slot.
3. Exact `FootClass::GetCurrentSpeed`, RawTrack metadata/accepted-chain initialization, and retry-visible speed-state semantics are not fully reconciled.
4. The complete Phase-1 population and current global post-effects do not yet have verified one-object owners.
5. No executable retail oracle certifies the state/order changes.

## Target Questions

### Checkpoint A: exact host and Mission_Move

1. What is the exact address-ordered Techno pre-mission sequence up to RockingUpdate, guard B, the remaining pre block, `+0xC4`, and Mission_Dispatch?
2. When Mission_Dispatch's timer has not elapsed, which Techno post work still runs in that same AI call?
3. For `CurrentMission=Move`, how does Mission_Dispatch select FootClass::Mission_Move, store its returned delay, and consume Scenario RNG?
4. Which object owns the RNG at global pointer `0x00A8B230`, what is the receiver frame for `+0x218`, and how many draws occur for the inclusive `RandomRanged(0,2)` path?
5. Which Mission_Move claims are counts in `g_CurrentFrameCounter` units versus measured wall-clock time? No count may be restated as seconds without runtime evidence.
6. What exact work precedes guard E, and what remaining Techno work follows it?
7. After Techno returns, what are Foot's alive check, five immediate locomotor gates, call slot, and post-Process alive check?
8. Which Unit/Infantry/miner special leaf paths bypass normal Foot/locomotor flow, especially active tube traversal?
9. What is the smallest truthful cloned-fixture/read-only event trace for an inert Mission_Move-to-Drive harness after this checkpoint?

### Checkpoint B: Drive speed and RawTrack exactness

10. What exact integer formula, widths, signedness, floating/integer conversions, rounding, clamps, and branch order does `FootClass::GetCurrentSpeed @ 0x004DB1A0` use?
11. Which type, owner-house, current-speed-fraction, veterancy/ability, mission, terrain, health, docking, and class-specific inputs participate for stock AMCV and MTNK?
12. On a same-Process DriveTrack retry, which speed-state writes and virtual calls repeat before fresh integer speed is masked?
13. What do each RawTrack metadata dword/byte and TurnTrack selector mean at every active read site?
14. What cursor/metadata values initialize fresh normal, accepted-chain, forced-track, short/reverse, and tube-related paths?
15. Which older field names or `entry_index - 1` claims survive receiver-base and callsite re-verification?

### Checkpoint C: complete Phase-1 population

16. For Walk, Hover, and Ship, what is the exact one-object Process entry, idle invocation behavior, movement/vertical ordering, tube producer precedence, completion owner, and post-Process alive behavior?
17. Where do Unit and Infantry active-tube leaf branches run relative to Techno/Mission/Foot and their ordinary locomotor Process?
18. Where does forced Drive track work run relative to ordinary Drive Process, NavCom, mission state, and refinery/dock leaf logic?
19. How can the current miner snapshot/process/writeback pipeline become a one-live-miner mission handler without moving radio, dock, credit, RNG, or unload effects to a later global point?
20. Is Rust's `sync_formation_speeds` equivalent to an active native convoy/team mechanism? If so, which object/team turn owns each mutation; if not, classify it DRIFT.

### Checkpoint D: lifecycle and gameplay-bearing effects

21. Which exact native point owns occupancy remove/add, cache invalidation, accepted-cell marking, path/head-to changes, PerCellProcess, scatter, crush, UnInit, sound, arrival, wall crush, gate contact, and war-factory exit contact?
22. Which effects must finish before the next LogicVector object, and which are positively proven global tail services?
23. How does native UnInit/conceal/unregister interact with the later pending-delete drain when movement kills a victim?
24. Can any current Rust deferred vector/list remain gameplay-bearing after the atomic flip? Every retained list needs positive proof that later objects cannot observe the delay.
25. What exact cache generation invalidations are necessary after each immediate effect?

### Checkpoint E: executable oracle

26. What retail capture method can record entry/exit state at the named native functions without changing gamemd behavior?
27. Can the oracle record object order, frame value, mission/timer, locomotor state, coordinates/facing, occupancy/lifecycle, and Scenario RNG cursor for the required fixtures?
28. What runtime cadence/jitter is measured for stock local GameSpeed values? This is a measurement follow-up only; it must not reopen the proven no-15-Hz-Drive-gate mechanism.

## Prior Work and Duplication Check

| Existing source | Current use | Gap this plan owns |
|---|---|---|
| [OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md) | Authoritative static owner/call schedule | Do not redo; use as fixed spine and only measure runtime timing in the oracle phase. |
| [TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_GHIDRA_REPORT.md) | Verified body map, guard B/E placement, Scenario RNG evidence | Re-verify load-bearing instruction ranges while building the exact host contract; reconcile simplified design/Rust bracket. |
| [TECHNOCLASS_AI_UPDATE_BODY_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_AI_UPDATE_BODY_SYNTHESIS.md) | Navigation summary | Derivative only; correct any flattened pre/post interpretation in the new contract report. |
| [FOOTCLASS_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_AI_GHIDRA_REPORT.md) | Broad Foot body navigation | Re-verify only the post-Techno alive check, locomotor gates/call, post-Process alive check, and active tube/special bypasses. |
| [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) | Mission/timer navigation | Two-pass audit and correction are mandatory before it can be cited as authority. |
| [S2_MISSION_DISPATCH_VS_PASSIVE_ACQUIRE_ORDERING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/S2_MISSION_DISPATCH_VS_PASSIVE_ACQUIRE_ORDERING.md) | Confirms passive acquire after dispatch | Reconcile with guard E placement and current Rust's partial post helper. |
| [DRIVE_PROCESS_DRIVE_TRACK_SPEED_BUDGET_RESIDUAL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_PROCESS_DRIVE_TRACK_SPEED_BUDGET_RESIDUAL_GHIDRA_REPORT.md) | Point cost, strict bound, residual, speed branch | Extend only into exact GetCurrentSpeed and retry pre-budget side effects. |
| [DRIVE_APPLY_TRACK_DELTA_POINT_RESIDUAL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_APPLY_TRACK_DELTA_POINT_RESIDUAL_GHIDRA_REPORT.md) | Apply_Track_Delta and accepted-chain claims | Re-verify disputed RawTrack names, receiver bases, and initializer values. |
| [DRIVE_TRACK_TABLES_DEEP_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_TRACK_TABLES_DEEP_DECODE.md) | Raw bytes and older role names | Treat role prose as conflicted until every active read is re-derived. |
| [WALK_LOCOMOTION_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WALK_LOCOMOTION_CLASS_GHIDRA_REPORT.md) and [E2_STATIC_WALL_WALK_RETRACE_20260720.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/traces/E2_STATIC_WALL_WALK_RETRACE_20260720.md) | Walk process and visible current drift | Use as coverage map; research only scheduler/owner/readiness gaps, not full A* parity. |
| `HOVER_LOCOMOTION_CLASS_GHIDRA_REPORT.md` | Recently re-verified Hover algorithms and Rules offsets | Verify one-object integration, idle invocation, vertical placement, and tube precedence. |
| [SHIP_LOCOMOTION_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHIP_LOCOMOTION_CLASS_GHIDRA_REPORT.md) | Full Ship class map | Selectively re-verify active Process/track/tube/completion ownership and current Rust mapping. |
| low-bridge TubeClass reports under `docs/research/bridges/04-locomotion-height-tubes/` | Active tube producers/consumers | Reconcile leaf early paths and atomic population precedence; do not conflate TS subterranean paths. |
| [CONVOY_FORMATION_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CONVOY_FORMATION_SYSTEM_GHIDRA_REPORT.md) | Older convoy/team map | Re-audit only the current Rust formation-speed mutation's alleged native owner and active stock reachability. |
| miner system model/reports under `docs/research/miner/` | Harvest/dock/radio state machines | Prove one-object extraction and exact same-pass ordering without redoing settled dock formulas. |
| [CRUSH_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CRUSH_SYSTEM_GHIDRA_REPORT.md), PerCellProcess reports, and object lifecycle synthesis | Movement-visible kill/lifecycle map | Reconcile immediate per-object effects with Rust's deferred removal and global tail. |

Before execution, compare modification times and search for newer reports covering the same exact questions. Extend newer work rather than duplicate it.

## Function Inventory

Addresses are navigation anchors, not trusted labels. The executor must confirm function boundaries, receiver frame, callsites, vtable bytes where load-bearing, and active-YR reachability. `VERIFY` means a bounded re-check; `DEEP` means the function is a primary unresolved target.

| Phase | Address / symbol | Role to establish | Depth |
|---|---|---|---|
| A | `Main_Game @ 0x0048CCC0` | One Main_Tick per outer iteration; fixed reference only | VERIFY |
| A | `Main_Tick @ 0x0055D360` | One late live pass and late frame increment/terminal exit | VERIFY |
| A | live-object pass `@ 0x0055AFB0` | Forward live-vector call and mutation semantics | VERIFY |
| A | `UnitClass::AI @ 0x007360C0` | Normal Foot call plus tube/special early paths | DEEP |
| A | `FootClass::AI @ 0x004DA530` | Techno call, alive checks, five pre-Process gates, `+0x40` call | DEEP |
| A | `TechnoClass::AI_Update @ 0x006F9E50` | Segmented pre/dispatch/post body and guard B/E positions | DEEP |
| A | `MissionClass::Mission_Dispatch @ 0x005B3060` | Timer gate, mission vtable dispatch, returned-delay storage | DEEP |
| A | `FootClass::Mission_Move @ 0x004D4200` | NavCom/moving/queue/arrival branches and jitter return | DEEP |
| A | `MissionClass::GetMissionTimerEntry @ 0x005B3A00` | `[Move]` timer-table lookup and units | DEEP |
| A | `MissionClass::Read_INI @ 0x005B3760` | Rate parsing and stored representation | VERIFY |
| A | `RandomClass::RandomRanged @ 0x0065C7E0` | Receiver identity and inclusive range consumption | VERIFY |
| A | Scenario pointer `0x00A8B230`, receiver `+0x218` | Prove Scenario RNG owner; reject Rules/noncritical label | DEEP |
| B | `FootClass::GetCurrentSpeed @ 0x004DB1A0` | Exact integer speed formula and every branch | DEEP |
| B | callees reached from `0x004DB1A0` | House bonus, mission/type speed, ability/veterancy, ftol helpers; resolve addresses from body | DEEP |
| B | `TechnoClass::SetSpeedFraction @ 0x004D3710` | Clamp/store widths and caller order | VERIFY |
| B | `DriveLocomotionClass::Process @ 0x004B0500` | Normal/no-track/retry branch ownership | VERIFY |
| B | `Process_Drive_Track @ 0x004B0F20` | Pre-budget writes, GetCurrentSpeed call/mask, metadata reads | DEEP |
| B | `Process_Movement @ 0x004B2630` | Fresh normal initializer, target fraction, accepted path | DEEP |
| B | `Apply_Track_Delta @ 0x004B0AD0` | Jump/mark metadata reads and receiver-base normalization | DEEP |
| B | `Force_Track @ 0x004B0C40` | Forced initializer, cursor, local destination, Apply_Track_Delta order | DEEP |
| B | `Can_Use_Track @ 0x004B4B00` | Track eligibility and raw metadata consumers | DEEP |
| B | Drive transform helper `@ 0x004B4780` | Raw-point transform frame used by jump/mark path | VERIFY |
| C | `WalkLocomotionClass::Process @ 0x0075AC80` | Per-object/idle entry and top-level order | DEEP |
| C | `WalkLocomotionClass::ProcessMovement @ 0x0075AEC0` | Sub-cell, tube producer, completion/effect order | DEEP |
| C | `HoverLocomotionClass::Process @ 0x00514310` | XY/facing/process order and idle behavior | DEEP |
| C | Hover `SpeedUpdate @ 0x00515ED0` | Speed-state owner and call placement | VERIFY |
| C | Hover vertical controller `@ 0x00513D20` | XY-versus-height/bob order and idle invocation | DEEP |
| C | `ShipLocomotionClass::Process @ 0x0069FC10` | Per-object/idle entry and slope/track order | DEEP |
| C | Ship `Process_Movement @ 0x006A1C80` | Track selection, speed, completion | DEEP |
| C | Ship `Process_Drive_Track @ 0x006A05F0` | Track/tube/cell effects and cursor order | DEEP |
| C | `UnitClass::TubeMovement @ 0x007359F0` | Active tube leaf owner, exit effects, Foot bypass | DEEP |
| C | `InfantryClass::AI @ 0x0051BAB0` | Mission/Walk/tube leaf order; the older `0x0051BF00` label is a mid-body address | DEEP |
| C | infantry tube routine `@ 0x0051B350` | Tube cursor/exit/placement effects | DEEP |
| C | `UnitClass::Mission_Harvest @ 0x0073E5E0` | One-live-miner mission ownership and transitions | DEEP |
| C | `FootClass::Mission_Enter @ 0x004D9290` | Miner/refinery timer/radio path in the live slot | VERIFY |
| C | `UnitClass::Mission_Deploy_Building @ 0x0073D630` | Dock/unload/forced-exit order relevant to miner handler | VERIFY |
| C | convoy clear `@ 0x006EC3A0` | Active convoy state mutation and object owner | DEEP |
| C | team convoy target `@ 0x006E9050` and callers | Determine whether Rust formation sync maps to team AI | DEEP |
| D | Unit per-cell dock hook `@ 0x00739EC0` | Accepted-cell/radio/contact order | DEEP |
| D | Unit crush PerCellProcess `@ 0x00741700` | Crush/scatter/kill order; verify receiver/role separately | DEEP |
| D | `ObjectClass::UnInit @ 0x005F65F0` | Conceal/unregister/pending-delete lifecycle | DEEP |
| D | `FootClass::Stop_Moving @ 0x004DF0D0` | Arrival/stop state clearing | VERIFY |
| D | `FootClass::Set_Destination_Internal @ 0x004D94B0` | NavCom/locomotor handoff and null teardown | VERIFY |

The inventory has fewer than 50 primary entries. If call expansion creates more than 50 load-bearing targets, stop and split the remaining work into a new approved investigation plan rather than broadening this one silently.

## Investigation Phases

### Phase 0: identity, freshness, and claim inventory

1. Confirm the active Ghidra program is the retail `gamemd.exe`, PE x86, image base `0x00400000`.
2. Record current modification times for every prior-work document and search for newer exact-topic reports.
3. Enumerate every load-bearing claim in [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) before editing it.
4. Classify each claim as CONFIRMED, WRONG, STALE, or UNVERIFIABLE against current binary evidence.
5. Do not trust local Ghidra labels. Verify boundaries, receiver pointers, calls, vtable slots, and data references.

### Phase 1: exact Techno/Mission/Foot contract and Mission_Move audit

1. Reconstruct `TechnoClass::AI_Update` in address order, marking:
   - pre prefix through RockingUpdate;
   - guard B and its exact exit;
   - remaining pre work;
   - `+0xC4` increment and Mission_Dispatch;
   - passive acquire, bomb, SlaveManager, CaptureManager;
   - guard E and its exact exit;
   - remaining post work.
2. Trace Mission_Dispatch's timer-not-ready and timer-ready paths. Record mission slot selection, returned delay, start/rate writes, and same-call return to Techno post work.
3. Trace FootClass::Mission_Move from dispatch through every branch, including NavCom, ILocomotion `+0x10 Is_Moving`, queued mission, OnArrival, timer lookup, and RNG call.
4. Prove the RNG owner from the global load, pointer dereference, receiver adjustment, and RandomRanged call—not from an inherited name.
5. Reconstruct the exact Foot tail from Techno return through alive check, the concrete pre-Process gates, locomotor `+0x40`, immediate alive check, and later eligible work.
6. Trace Unit/Infantry special early paths that bypass this normal spine, especially active tubes.
7. Compare the result to current `unit_techno_bracket` and enumerate every missing/misplaced behavior without proposing Rust code.
8. Correct [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) immediately after facts are verified, including its RNG-owner and cadence language, and record the audit result according to the project's audit workflow.
9. Produce [docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md) with a Rust handoff limited to an inert/cloned-fixture harness boundary.

#### Checkpoint A exit criteria

**Verdict: PASS (2026-07-20).** The corrected reports satisfy every criterion below. The separately cold-reviewed inert harness plan is `docs/plans/2026-07-20-ordinary-drive-inert-host-harness-plan.md`; this does not authorize production activation.

- Guard B/E positions are byte-backed and not represented as one generic pre/post guard pair.
- Mission_Dispatch-to-Mission_Move timer and return-delay semantics are exact.
- Scenario RNG receiver and draw count are exact.
- Foot post-Techno, pre-Process, and post-Process gates are exact.
- Special leaf bypasses are enumerated.
- The stale Mission_Move report is corrected and no remaining load-bearing claim is silently unknown.
- Only after review may a separate `/write-plan` target an inert/test-only harness; no production activation is authorized.

### Phase 2: exact Drive speed and RawTrack metadata

1. Fully decode `FootClass::GetCurrentSpeed`, including each callee and conversion. Record every width, signed comparison, float/double operation, ftol boundary, integer multiply/divide, clamp, and class-specific branch.
2. Prove input provenance for stock AMCV and MTNK from `rulesmd.ini`, owner house, veterancy/ability state, speed fraction, terrain/health, and mission state.
3. Walk concrete AMCV and MTNK fixtures through the formula using binary-derived inputs. Label results derived; do not substitute hand-computed outputs for the executable oracle.
4. Re-check both DriveTrack call sites and prove the retry repeats pre-budget speed-state work before masking fresh integer speed.
5. Inventory every read of RawTrack and TurnTrack metadata in the five named Drive functions. Normalize object base versus ILocomotion subobject base before naming a field.
6. Build a table for fresh normal, accepted-chain, forced, short/reverse, and tube-related initialization: selector, cursor, residual, head-to, destination, marking, and first consumable point.
7. Reconcile [DRIVE_TRACK_TABLES_DEEP_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_TRACK_TABLES_DEEP_DECODE.md), [DRIVE_APPLY_TRACK_DELTA_POINT_RESIDUAL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_APPLY_TRACK_DELTA_POINT_RESIDUAL_GHIDRA_REPORT.md), and the July 20 traces. Mark stale prose explicitly.
8. Produce:
   - `docs/research/FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md`;
   - [docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md).

#### Checkpoint B exit criteria

- Exact GetCurrentSpeed formula and stock fixtures have no open width/rounding/modifier question.
- Retry speed-state versus fresh-integer behavior is unambiguous.
- Every active initializer has a sourced cursor/metadata contract.
- Raw bytes and role names agree across current authoritative docs, or unresolved roles remain explicitly BLOCKED.

### Phase 3: complete active ground population and branch precedence

1. For Walk, Hover, and Ship, produce one invocation-order table each: leaf entry, idle gates, movement path, tube/special path, cell effects, completion, vertical/slope work, and alive checks.
2. Verify stock YR locomotor CLSIDs/types that activate each branch. Separate stock-active, conditional/mod-only, and dormant TS paths.
3. Prove Unit and Infantry tube movement's leaf-level precedence relative to Techno/Mission/ordinary locomotor flow.
4. Prove forced Drive track precedence and all active callers relevant to refinery exits, deployment, and scripted movement. Do not generalize the fresh-normal cursor.
5. Map the native Harvest/Enter/dock/unload path into a one-live-miner ordered contract. Reuse existing miner reports for settled formulas; focus on snapshot removal, same-pass visibility, and authority handoff.
6. Re-audit the native convoy/team mechanisms against Rust `sync_formation_speeds`. Classify the Rust mutation as MATCH, DRIFT, or UNCHECKED, and name its native owner if one exists.
7. Compare against current Rust category routing and list every Phase-1 entity state that the future ground dispatcher must process exactly once.
8. Produce [docs/research/GROUND_PHASE1_LOCOMOTOR_POPULATION_AND_PRECEDENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_PHASE1_LOCOMOTOR_POPULATION_AND_PRECEDENCE_GHIDRA_REPORT.md).

#### Checkpoint C exit criteria

- Ordinary Drive, miner, Infantry/Walk, Hover, Ship, forced track, and active tube each have one exact native entry/precedence contract.
- Idle/no-MovementTarget invocation behavior is covered.
- No active category relies on a later global mutation with unknown ownership.
- Dormant Tunnel/Mech/DropPod code is not substituted for active YR tube behavior.

### Phase 4: lifecycle, effects, cache visibility, and postlude ownership

1. Start from every gameplay-bearing action currently in and immediately after `tick_movement_with_grids`:
   - NavCom target refresh;
   - tube and forced-track work;
   - blocker/cache construction and refresh;
   - pending Drive arrivals;
   - point/cell/occupancy effects;
   - deferred chain and occupancy checks;
   - scatter RNG and crush kills;
   - formation synchronization;
   - finished-movement finalization;
   - locomotor phase updates;
   - Hover vertical work;
   - gate runtime, war-factory exit contacts, and wall crush.
2. For each action, identify the native caller and exact placement: current object point, leaf post-work, another object's turn, or proven global tail service.
3. Trace one crush fixture through PerCellProcess, sound/RNG, UnInit, live-vector removal, occupancy, and pending-delete drain. Record what the next object sees.
4. Trace one nonlethal occupancy contention fixture, one scatter fixture, one arrival fixture, and one gate/factory-contact fixture.
5. Specify cache invalidation facts after occupancy, lifecycle, wall, bridge, or passability mutations. Do not infer equivalence from current generation counters alone.
6. Classify every current deferred vector/list. A gameplay-bearing deferral with no binary proof is DRIFT.
7. Produce [docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md).

#### Checkpoint D exit criteria

- Every current global/postlude mutation has a verified owner or an explicit blocker.
- Later-object visibility is exact for position, occupancy, lifecycle, RNG, navigation, wall/gate/factory contact, and sound/event order.
- The eventual atomic caller removal boundary is mechanically enumerable; no production handled-ID bridge is needed.

### Phase 5: executable native oracle and runtime measurements

1. Select a read-only capture method that observes retail state without changing game logic. Document tool, build identity, addresses/symbols, sampling points, and capture limitations.
2. Capture at least:
   - AMCV open-ground turn from rest;
   - MTNK straight movement with spendable residual across a cell boundary;
   - track completion plus same-Process retry;
   - empty-queue arrival;
   - two ground movers contending in known LogicVector order;
   - one crush or scatter lifecycle interaction;
   - one Infantry/Walk case;
   - one Hover case including vertical state;
   - one Ship or active tube case;
   - one miner mission-to-locomotor transition.
3. For every invocation, record object ID/order, observed frame, mission/substate/timer, locomotor kind/gates, coordinate/facing, track/cursor, speed fractions, fresh integer, residual, NavCom/head-to/path, occupancy/lifecycle, and Scenario RNG cursor where relevant.
4. Capture local GameSpeed timer intervals separately and label achieved throughput/jitter as measured. Do not translate this into a Drive-specific divisor.
5. Define the machine-readable fixture format and how Rust will later consume it. Hand-computed expected values are not acceptable.
6. Produce `docs/research/GROUND_MOVEMENT_EXECUTABLE_NATIVE_ORACLE_CAPTURE_REPORT.md` plus capture artifacts in the project's approved evidence location.

#### Checkpoint E exit criteria

- Required fixtures are retail-derived and reproducible.
- Capture fields cover every acceptance comparison in the design/contract.
- No VERIFIED parity claim depends only on Rust-vs-Rust hashes, prose, or hand computation.

## INI and Retail-Data Checklist

Read YR `*md` data first, then base fallback:

- `[MultiplayerDialogSettings] GameSpeed=` for pacing context only.
- `[Move] Rate=` and the mission timer-table reader; preserve stored count units.
- Stock `[AMCV]` and `[MTNK]`: `Speed`, `ROT`, `Locomotor`, `MovementZone`, `SpeedType`, `Accelerates`, `AccelerationFactor`, `DeaccelerationFactor`, `SlowdownDistance` where inherited/overridden.
- Representative Walk infantry, Hover vehicle, Ship/naval unit, harvester/miner, and tube-capable map fixtures.
- `[General] HoverHeight`, `HoverBob`, `HoverDampen`, `HoverBoost`, `HoverAcceleration`, `HoverBrake`.
- `[AudioVisual] Gravity` for Hover vertical behavior.
- Retail map TubeClass facts and explicit `[Tubes]` only where the chosen fixture actually contains them.
- Formation/convoy keys or type flags only after their binary readers are verified; do not infer a native `group_id` from the Rust field.

Record exact section/key origin, base-versus-YR override, parsed type, default, and binary consumer. Do not import stock behavior from an external mod repository.

## Current Rust Integration Surface to Compare

Read-only comparison targets during research:

- `src/sim/world/mod.rs`: `advance_tick`, live-order owner, `uninit`, pending-delete drain, frame commit, Phase-1 and adjacent gate/contact/wall calls.
- `src/sim/world/techno_ai.rs`: `object_ai_stage`, `object_ai_walk`, `techno_common_pre`, `unit_techno_bracket`, `techno_common_post`, current debug shadows.
- `src/sim/movement/movement_tick.rs`: global pass prelude, mover collection, per-mover loop, deferred chain/occupancy, formation, crush, finalization, Hover vertical.
- `src/sim/movement/movement_step.rs`: explicit three-subtick Drive gate, retry helpers, cell-event boundary, Infantry/generic steps.
- `src/sim/movement/drive_locomotion.rs`, `drive_track.rs`, `movement_commands.rs`, `navcom.rs`.
- `src/sim/movement/tube_movement.rs`, `hover.rs`, `locomotor.rs`, and Ship/Walk behavior embedded in generic movement code.
- `src/sim/miner/miner_system.rs`, `miner_dock_sequence.rs`, and related radio/dock state.
- `src/sim/movement/bump_crush.rs`, occupancy helpers, `src/sim/gate_runtime.rs`, `src/sim/production/war_factory_exit.rs`.
- `src/sim/components.rs`, `world_hash.rs`, `snapshot.rs` for canonical/persisted authority.
- `src/app_sim_tick.rs`, `src/app_types.rs`, and frame consumers for the already-proven scheduler/frame delta.

Expected future architecture surfaces from the approved design are navigation aids only during research: a transient `src/sim/movement/ground_pass.rs`, one-object locomotor entry seams, and immediate effect commits. This plan does not authorize creating them.

## Edge Cases and Adversarial Checks

- Null locomotor, idle Drive, no MovementTarget, dying/concealed owner, deployed/limbo/gated Foot states.
- Mission timer not elapsed versus elapsed; queued mission; Mission_Move stopped/moving/NavCom-null combinations.
- Mission/manager work that kills the owner at guard B or E.
- Locomotor Process that kills/conceals the owner before Foot's immediate alive check.
- Same-pass tail append and compacting removal of an object at/before the current live index.
- Drive fresh normal, zero budget, exact budget 7, residual 8, accepted-chain, forced track, same-Process retry.
- Retry repeats speed-state work but masks fresh integer speed.
- Active tube state at Unit/Infantry leaf entry; zero-length automatic low-bridge tube shell versus explicit nonzero tube.
- Hover moving and idle bob/height; unpowered/sink branch if stock-reachable.
- Ship first-tick flag, slope state, stop/deceleration, tube path.
- Miner entering, docked/unloading, exiting, radio-link loss, death during mission.
- Crush victim before/after current index, scatter RNG, sound coordinate/order, pending deletion.
- Formation member death/removal, mixed speeds, no native convoy link.
- Save/load immediately before a Drive invocation remains a deferred serialization follow-up unless Phase 5 can capture it without widening scope.

## TS-Legacy and Active-YR Guardrails

- Active low-bridge TubeClass traversal is in scope.
- Subterranean Tunnel, Mech, and DropPod locomotion are stock-dormant TS legacy and are not substitutes for tubes.
- If Mech code appears as a shared tube-state producer, record the shared mechanism but do not classify it stock-active without YR reachability evidence.
- Fog/shroud behavior is unrelated and must not enter this investigation.
- Stock/mod-only distinctions must be recorded for every locomotor/type gate.

## Evidence and Citation Discipline

For every new binary claim:

1. cite the exact read-only Ghidra action inline (`decompile_function`, `disassemble_function`, `get_function_callers`, `get_xrefs_to`, `read_memory`, or equivalent);
2. record address range, receiver frame, argument flow, and active-YR call path;
3. separate VERIFIED findings from inference and UNKNOWN/UNCHECKED items;
4. reconcile conflicts against function bodies and callsites, not local labels;
5. update the relevant research document immediately after verification;
6. never certify parity from an old plan, prior Rust hash, or hand-computed fixture.

## Deliverables

Required research outputs after execution:

1. corrected [docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) plus audit record;
2. [docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md);
3. `docs/research/FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md`;
4. [docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md);
5. [docs/research/GROUND_PHASE1_LOCOMOTOR_POPULATION_AND_PRECEDENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_PHASE1_LOCOMOTOR_POPULATION_AND_PRECEDENCE_GHIDRA_REPORT.md);
6. [docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md);
7. `docs/research/GROUND_MOVEMENT_EXECUTABLE_NATIVE_ORACLE_CAPTURE_REPORT.md` and machine-readable native captures;
8. updated design/implementation contract only if verified evidence changes a premise or closes a blocker.

Each report must contain a coverage ledger, open-question final state, current Rust comparison, implementation handoff, negative facts, and exact source ledger.

## Completion and Stop Conditions

The investigation is complete only when:

- every target question is RESOLVED or explicitly DEFERRED with category, reason, and next owner;
- the Mission_Move report is corrected and its RNG/timing claims no longer conflict with the binary/scheduling report;
- the exact host spine is sufficient to review a truthful inert harness plan;
- GetCurrentSpeed and RawTrack initializer contracts have no silent guessed value;
- every Phase-1 ground category and gameplay-bearing post-effect has a verified owner/precedence or remains an explicit production blocker;
- executable native fixtures exist for the named acceptance surfaces;
- the design and implementation contract reflect the final evidence;
- no Rust or production behavior was changed during research.

Stop early and report rather than guessing if:

- the active Ghidra program is not the retail target;
- a function boundary/receiver cannot be established;
- the scope expands beyond 50 load-bearing functions;
- runtime oracle capture requires mutation or tooling authority not yet granted;
- evidence invalidates the approved atomic architecture premise.

## Execution Strategy

Execute sequentially at the checkpoint boundaries:

1. **Completed:** Phase 0-1 and cold review of the corrected Mission_Move document and exact host contract.
2. **Completed:** the separately requested inert/test-only harness plan states that production state/hash is unchanged and stops at Foot return.
3. Execute Phases 2-4 as bounded `/re-investigate` slices, updating the contract after each closed blocker.
4. Execute Phase 5 once capture tooling/fixture setup is available.
5. Request a design/contract review before any production implementation plan.

No production Rust implementation should begin from this document alone.
