# Ghidra Typing Corridor Program

> **What this document is.** The frozen method and ranked corridor list for applying
> verified data types, prototypes, and receiver types to the Ghidra program for
> `gamemd.exe`. It freezes **order and method, never status** — whether a corridor is
> done is re-derived each session from live Ghidra state (is the type applied? does the
> decompile render it?), never recorded here. A disposition ledger exists only inside
> each tier's own goal (see protocol), scoped to that tier's rows.
>
> **What this document is not.** Not a completion tracker (ENGINE.md forbids those),
> not a parity claim, not standing authorization. Every tier requires its own goal
> prompt from the user carrying explicit per-task authorization for its scoped edits.
>
> Ranking evidence (2026-08-17 scan): Rust provenance-comment citations per class,
> phase order in `2026-07-30-clean-slate-system-implementation-order.md`, and the
> active goal stream (Phase 5 movement/shroud). Re-rank freely between tiers as the
> goal stream moves; do not re-rank mid-tier.

## Why corridors, not coverage

Typing pays only where port sessions read. Corridors are run **on demand** — when the
goal stream is about to live in one — never as a sweep toward full coverage. If a
corridor never earns a session, it never gets labels. The program has no completion
point; it thins out as the land-as-you-go habit (each port session applies the fields
it verified anyway, under per-task authorization) absorbs the standalone tiers.

## Shared protocol (every tier)

The proven skeleton is the MissionClass lifecycle tier prompt (2026-08-17). Each tier
prompt = this protocol + a finite scope block. Non-negotiable clauses:

1. **Authorization.** The tier prompt states: "This prompt is the explicit per-task
   authorization ENGINE.md requires for prototype, receiver-type, and datatype edits —
   for the scoped rows only," with explicit non-grants (no function creation, no edits
   outside the frozen ledger, no renames unless separately scoped).
2. **Named authority.** Active `gamemd.exe`: exact bytes, RTTI, vtables,
   callers/callees, x86 ABI — under AGENTS.md and ENGINE.md. Class reports, Rust
   provenance comments, and YRpp-shaped names are leads, never evidence.
3. **Snapshot or read-only.** Before any mutation: recoverable snapshot of `.gpr` +
   `.rep` with Ghidra safely closed. No snapshot → ledger/report only, zero mutations.
4. **Frozen finite ledger.** Read-only trace proves every scoped row (target, owner,
   convention, return, parameters/storage, receiver, active-YR relevance, datatype
   evidence) before any write. Later discoveries become bounded residuals. Ledger
   location: the tier's doc under `docs/research/`; live Ghidra state is the authority
   on applied-ness, the ledger on scope and evidence.
5. **Critic before write.** A fresh read-only critic verifies each APPLY row's cited
   evidence without the builder's reasoning. Simple field tables may instead use
   writer-verifies-every-row (the writer independently confirms cited evidence at
   apply time); vtable/prototype corridors keep the separate critic — that is where
   the project has paid for an error before (thunk-slot trap).
6. **Serial writes, save+readback per row.** One writer; readers stopped. After each
   mutation: `save_program`, read back, confirm against the ledger row. Failed
   readback → stop, restore snapshot, mark REJECTED, fresh critic re-passes prior
   rows. Receiver types associate last (moves symbols into class namespaces).
7. **Non-destructive only.** `void *`/`undefined` for unresolved semantics; never
   delete/recreate/shrink structures; never auto-analysis, re-import, or broad
   renames. Holes stay holes — unproven fields are recorded, not guessed.
8. **Done-clause.** Every row APPLIED / VERIFIED INHERITED-NO-OP / REJECTED / bounded
   RESIDUAL; writes persisted and read back; report records evidence, snapshot path,
   residuals. Stop for user review before any further tier. No Rust edits, commits,
   or pushes.

## Ranked corridors

Traffic = provenance-comment citations in `src/` (2026-08-17 grep, undercounts —
comments citing bare addresses aren't caught). Trigger = when to run the tier.

| # | Corridor | Scope sketch | Evidence / traffic | Trigger |
|---|----------|--------------|--------------------|---------|
| 1 | **Mission lifecycle vtable slots** | 7 slots × 6 classes (≤42 rows); baseline enum + base prototypes exist | Mission authority ported; handler absorption active | Applied 2026-08-17 (Claude Code, lean supervised run) — see [MISSIONCLASS_BASE_PROTOTYPES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSIONCLASS_BASE_PROTOTYPES_GHIDRA_REPORT.md) "Tier 1 corridor outcome"; verify live before extending. Codex thread `01a00f6b` superseded — do not resume |
| 2 | **CellClass fields** | Sim-touched fields: passability, occupier chain, overlay/ore, shroud, bridge, height | Highest provenance traffic (8 files); rich label corpus (ore/passability fns) | Applied 2026-08-17 (14 fields + 11 typed receivers) — see CELLCLASS_STRUCT_GHIDRA_REPORT.md "Tier 2 application record"; residuals listed there (0x11A conflict, ~66 receivers on-contact) |
| 3 | **Locomotor interface (ILocomotion) slots** | The vtable corridor movement ticks dispatch through; Drive/Ship/Hover/Fly/Jumpjet receivers | movement_tick/movement_step provenance; piggyback mechanism recurring | Hot-path slice applied 2026-08-17 (Drive+Walk slots 4/16/17/18/32, __stdcall prototypes) — see [ILOCOMOTION_COM_PROTOCOL_SPEC.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ILOCOMOTION_COM_PROTOCOL_SPEC.md) "Tier 3 application record"; other families/slots on contact; Walk Move_To mislabel flagged |
| 4 | **FootClass fields** | NavCom/TarCom, path state, team link, speed/coord accumulators | FootClass__AI + pathfinding traffic | Applied 2026-08-17 (struct created, 41 fields, 7 receivers) — see [FOOTCLASS_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_STRUCT_LAYOUT.md) "Tier 4 application record"; TechnoClass region empty pending tier 6 |
| 5 | **ObjectClass core slots + fields** | Update/Mark/Limbo/Unlimbo lifecycle slots; coord, health, layer fields | lifecycle.rs traffic; every later phase inherits | Applied 2026-08-17 (30 fields, 24 prototypes, 20 receivers, 8 label corrections, 2 new datatypes) — see [OBJECTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_GHIDRA_REPORT.md) "Tier 5 application record"; holes and residuals listed there |
| 6 | **TechnoClass sim fields** | Target/ammo/veterancy/cloak/owner cluster (extend+verify the sparse YRpp-imported struct; imported names re-derived from asm before kept) | combat/mod.rs, snapshot.rs; struct exists 1312 B, ~45 unverified fields | Applied 2026-08-17 (9 rows: 3 refuted names, 2 real type upgrades, 3 new fields, 1 scope fix; 5 holes) — see [TECHNOCLASS_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_STRUCT_LAYOUT.md) "Tier 6 application record". Residual: the live struct is a FLAT import that repeats base rows inline while MissionClass embeds ObjectClass — the two shapes disagree; reconcile deliberately, not mid-tier |
| 7 | **Weapon-fire corridor** | Fire/CanFireAt/Assign_Target slot set + BulletClass fields | TechnoClass fire corpus labeled | Applied 2026-08-17 (new 352-B BulletClass struct + 24 fields, 6 fire-path prototypes with receivers, 6 holes) — see [BULLET_CLASS_LAYOUT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_LAYOUT_GHIDRA_REPORT.md) and the plate comments on 0x00466380 / 0x006FC0B0. Residuals: InfantryClass fire-slot binding UNPROVEN (candidate 0x0051DF70 has no xrefs); 0xB4-0xCF uncharacterized |
| 8 | **UnitClass + InfantryClass fields** | Harvester state, deploy, sub-cell position | pathfinding/movement/harvest traffic | Applied 2026-08-17 (UnitClass 2280 B / 12 fields, InfantryClass 1776 B / 7 fields, CDTimerClass created; 3 negative findings) — see [INFANTRY_SUBCELL_POSITIONING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRY_SUBCELL_POSITIONING.md) "Tier 8 application record". **Sub-cell is NOT a field** — recomputed per call; **ore cargo is NOT a UnitClass field** — StorageClass at TechnoClass+0x33C, 4 slots |
| 9 | **HouseClass fields** | Credits/power/ownership arrays (scale-diverged storage: verify layout, type only native-frame reads) | miner_tests, scenario_bootstrap | Applied 2026-08-17. **The struct was resized 22,500 -> 90,296 B (0x160B8)** — the old size was wrong by ~4x; 13 fields added, 20 preserved, `StorageClass` created. **SCALE: kill arrays are `int[20]` house-indexed with NO bounds check (breaks at 21 houses); allies is a single dword = hard 32-house ceiling.** See the plate comment on 0x004F9610 and the residuals below |
| 10 | **BuildingClass + FactoryClass fields** | Factory state, dock links, power | anim_class traffic; factory ported (P5b/P5d) | Applied 2026-08-17, critic green with ZERO refutations. BuildingClass resized 1820->1824 (`SizeOf` = `MOV EAX,0x720; RET`); FactoryClass 116 verified correct. **HouseClass `Primary_For*` x6 hold `FactoryClass*`, not buildings** — renamed `FactoryQueue_For*`. Two swapped FactoryClass timer labels corrected. Production = 54-step INTEGER counter (divisor proven by the `0x4BDA12F7` magic constant = 2^36/54) |
| 11 | **AircraftClass + Fly/Jumpjet fields** | Flight state, landing, carryall | lifecycle FlyLocomotion cite | Applied 2026-08-17. AircraftClass 0x6D8=1752 (4 alloc sites + RTTI COL offset 0x6C0); **created `JumpjetLocomotionClass` 152 B and `FlyLocomotionClass` 96 B — neither existed**; **`DriveLocomotionClass` resized 108 -> 112 (third imported-size error)**. Jumpjet DISPATCH has exactly 7 outcomes (table at 0x0054B19C, 7 entries, verified) but the value set is **NOT provably closed** — the `CMP EAX,0x6; JA` is UNSIGNED and its default target is byte-identical to table entry 6, so any value >= 6 (including -1 as 0xFFFFFFFF) dispatches as state 6. A 7-variant port enum is safe ONLY if out-of-range maps to variant 6 rather than panicking. 3 destructor-vs-constructor mislabels corrected. **Negative findings: no landing-stage enum (uses MissionClass 0xBC), no carried-unit field (CargoClass at 0x114), dock link is a cached hint not a reservation** |
| 12 | **MapClass sim-relevant fields** | Cell array access, zone/passability caches | bridge_state traffic | Applied 2026-08-17 (struct created, 24 fields). **SCALE CEILING: the cell table is a fixed 512x512 pointer table; stride 512 hard-coded in 4 places, bound 0x40000 hard-coded in 2 of the 3 ACCESSOR checks, and `Width + Height <= 512` is the binding limit — a 256x256 map sits exactly AT it.** (The applier first derived 256 from the raw Resize loop bounds; a critic refuted it — the loop is clipped by the diamond guard. Corrected.) Enlarging capacity alone would let out-of-range indices pass silently, and the allocation write itself is unbounded, so overrun is a heap write rather than a clean failure. See the plate comment on 0x005657A0. MapClass is a STATIC GLOBAL at 0x0087F7E8 (folded absolute addresses prove it), and is the base subobject of a derived DisplayClass instance |
| 13 | **EventClass / command corridor** | Event union layout, per-type sizes | commands/lockstep/world_commands traffic | Applied 2026-08-17. Record = **0x6F (111 B)**, proven 4 ways; struct created; 47-opcode map with names read from the binary's own string table at 0x0082091C. **SIX DETERMINISM HAZARDS documented in the plate comment on 0x004C6CB0** — 104 uninitialised transmitted bytes in ABOUTTOEXIT, a 1-byte hole in MEGAMISSION, an executed-flag never cleared on insert, TIMING mutating its own record, ADDPLAYER carrying a raw heap pointer it `free()`s, and Frame being rewritten 3x so it is never sender-authoritative |
| 14 | **AI/team corridor** (TeamClass, TaskForce, TriggerClass, ScriptClass) | AI study DONE 2026-08-18 | script VM decoded; trigger handlers specified, not yet typed | **TIER 14 CLOSED 2026-08-18** — AI study + trigger/event/action/opcode subset applied and critic-verified. Original skeleton (2026-08-18) — all 7 class sizes proven from allocation sites and structs created (TeamClass 160, TeamTypeClass 248, TaskForceClass 212, TriggerClass 72, TriggerTypeClass 180, ScriptClass 48, ScriptTypeClass 564). Structural slots only; **behavioural roles deliberately NOT named** — that is the AI study's job. **8-wide limit on map-authored owner tokens** — `0x00510ED0` recognises exactly eight constants 0x117B..0x1182 -> 0..7, returning NULL otherwise (0x00510F4F), reached from TeamTypeClass+0xC8 via 0x006F2070. **The applier first called this "the concrete 30-player blocker"; the final critic REFUTED that framing.** The constants are the `<Player @ A>`..`<Player @ H>` map tokens (proved by the reverse name->id map 0x00510FB0 string-comparing against 0x00825000..0x00825070), there is a TWIN predicate at 0x00510F60 with the same chain and 17 callers, the downstream table at 0x0068C030 is bounded at **16** not 8, `EventClass+0x02` is a signed char house index (127-wide), and 0x00510ED0's own bound is the runtime `g_HouseClass_Array_Count`. So it genuinely blocks map-scripted team/trigger ownership past 8 slots and must be widened, but 30 players already fit the event and house-array plumbing. No player-indexed array exists inside any of the 7 classes; the fixed counts (TaskForce 6, TeamClass 6, ScriptType 50, TriggerType 3) are composition caps, not player caps — all three array counts critic-confirmed with exact size closure **AI STUDY (2026-08-18), 3 critic lanes, all under the 10% tripwire — structs 1/35 refuted, script VM 0/15, delivery-bar PARTIALLY CONFIRMED.** Created `TEventClass` 88, `TActionClass` 148, `TagClass` 56, `TagTypeClass` 164, `CellStruct` 4. **The trigger engine is NOT campaign-only** — `Main_Tick` read start to end shows the session global gates only timer/UI and the campaign and skirmish arms reconverge before the logic-tick call, so map ambience runs in ordinary skirmish (246 non-campaign maps, 89% of YR-mounted ones). **The applier overstated this first** — the corpus silently included 15 campaign maps, 261 map files back only 133 distinct trigger sections, "every map" is really 89%, "SILENT" is really "missing map ambience", the action-99 count was off by 8, and the dominant-action set omitted action 55. **One row refuted:** `TagClass+0x30` was neither a sentinel nor an `int` — it is a signed `CellStruct{X@0,Y@2}` fed to `MapClass::operator[]`. Seven field names demoted to holes (proven offset+width, no located consumer). **Script VM: 12 opcodes = 95.6% of stock steps.** op 0 is terminal by design; op 11 never advances (15 of 16 stock uses are a script's last step); **op 54 is the highest freeze risk** — no advance store in its callee at all and none of op 53's three escapes. Opcode 2's permanent stall has **0 stock occurrences** (latent); opcode 6's `arg-2` seek has **10** (live). **Aircraft silently drop mission orders** — `+0x1E8` is a filter on AircraftClass that can return without calling anything, and opcode 11 deliberately admits aircraft. Opcode 58's lo16 >= 300 arguments resolve **one position early** (cause unproven; do not "fix" it in a port). Remaining: 9 event + 10 action handlers specified but not individually typed, and 8 minor opcodes (20 of 458 steps). |
| 15 | **Named globals / enums** | Armor/land enums; `.bss` runtime globals (debugger-gated — values zero in static image) | BRIDGE_BSS sweep report lists candidates | **Partially satisfied 2026-08-17 as a by-product of tiers 5-13** — see the globals harvest below. VALUES remain debugger-gated (verified: 0x00A8ED84, 0x0087F924, 0x00A8022C all read as zeros), but a global's ROLE is provable from its writer/reader without a debugger, and ~16 were proven that way this session |

## Tier 15 globals harvest (2026-08-17, proven as a by-product of tiers 5-13)

**The static image cannot give a `.bss` global's VALUE** — verified this session: 0x00A8ED84
(frame counter), 0x0087F924 (cell-table base) and 0x00A8022C (house array) are all live in
ordinary play and all read as zeros from the file. **But a global's ROLE is provable from its
writer and readers without a debugger,** and that is what the corridor actually needs. Roles
proven while working other corridors, each cited in the tier record that found it:

| Address | Role | Proven in |
|---|---|---|
| 0x0087F7E8 | `MapClass` instance — a STATIC GLOBAL, not a pointer (compiler folds field accesses to absolute addresses) | tier 12 |
| 0x0087F924 | = 0x0087F7E8 + 0x13C — the cell-table base pointer | tier 12 |
| 0x00ABDC50 | shared dummy `CellClass` returned on lookup failure; never NULL | tier 12 |
| 0x0089E9F0 | sub-cell offset table — **runtime-written**, reads as zeros statically | tier 8 |
| 0x00A8ED84 | current frame counter — the clock every CDTimerClass compares against | tiers 6-13 |
| 0x008871E0 | `RulesClass` instance | tiers 6, 9, 10 |
| 0x00A8022C / 0x00A80238 | `HouseClass` array items / count | tier 9 |
| 0x00B0F4EC | `TiberiumClass` array — indexed by the 4 StorageClass slots | tier 9 |
| 0x0087F778 | LogicClass container (IsInLogic membership) | tier 5 |
| 0x00A8ECB8 / BC / C8 | current-selection vector items / data / count | tier 5 |
| 0x008A0364 | display layer array, stride 0x18, 5 layers | tier 5 |
| 0x0081F7B4 | CRC-32 table (reflected, poly 0xEDB88320) | tier 5 |
| 0x00AC1380 | zero-coordinate global used as the null-coord sentinel | tier 5 |
| 0x0087E294 | sound-event pool tag written into every VocHandle +0xC | tier 5 |
| 0x00A802D4 / 0x008B4204 / 0x00A83EDC | OutList / DoList / MEGAMISSION-regroup command rings, all stride 0x6F | tier 13 |
| 0x0082091C | 47-entry event opcode NAME table (`char*`) | tier 13 |

Still genuinely debugger-gated: anything whose *value* (not role) is load-bearing — e.g.
confirming a runtime table's contents rather than its writer. Those rows stay holes until the
debugger is wired, and that dependency is the honest blocker, not missing analysis.

## Final full critic pass — 2026-08-18 — GREEN

Three fresh adversarial agents re-verified the whole program from live Ghidra state, not from
the applier's notes.

- **All 28 class sizes CONFIRMED**, including the four inherited ones (ObjectClass 172,
  MissionClass 212, TechnoClass 1312, FootClass 1728) that nine corridors were built on without
  ever being proven. No offsets invalidated.
- **All 17 renames returned KEEP NEW — no destroyed name was correct.**
- **All three tier-14 array counts CONFIRMED** with exact size closure (TeamClass 6,
  TaskForce 6, ScriptType 50), all three reported mislabels confirmed genuinely mislabelled, and
  all ten cross-tier field spot-samples CONFIRMED.

Two applier claims were refuted and corrected in the same pass: the MapClass scale ceiling
(256 -> 512) and the tier-14 "30-player blocker" framing (it is an 8-wide limit on map-authored
`<Player @ X>` owner tokens; 30 players already fit the event and house-array plumbing).

**Caveats the final pass attached to otherwise-good rows — do not let these rot into
certainties:**
- `0x005F4240` What_Action_OnObject: the cited caller 0x00417BE1 lies in an undefined gap and
  does not resolve to a function. The identity is proven independently; that citation is not.
- `0x005F6CB0` Set_Custom_Sound: "Custom" is inherited from the imported struct field name, not
  proved from bytes, and the receiver binding is field-offset dataflow rather than a vtable slot.
- `0x0054AD00` JumpjetLocomotionClass__Destructor: **no xrefs at all** — no vtable slot, no
  caller. Body evidence is unambiguous but this sits below the project's active-caller-binding
  bar for a function name.
- `ObjectClass` 0x99 `IsDrawnThisFrame`: the old `IsVisible` was vague rather than demonstrably
  wrong. This rename bought the least of the seventeen.
- `TechnoClass` 0x2AC `DeployedFrom`: the reciprocal link with 0x2B0 is proven; the **direction**
  ("From" vs "Into") is inference from building-side usage and stays UNCHECKED.
- `MapClass` 4468 is a tight lower bound. It is a static global with no `Size_Of` and no
  allocation site, so nothing available excludes a trailing member the constructor never
  initialises. Everything at or below 0x1174 is sound; the terminal byte is UNCHECKED.

**Port-relevant finding surfaced by the rename audit:** ObjectClass +0x84 gates the fall-rate cap
between RulesClass+0x7B8 (parachute) and +0x7BC (no parachute) — so gamemd applies the parachute
fall rate to **any falling object with any anim attached**, not only parachuted ones. That is why
the old `HasParachute` name looked right, and it is behaviour a port must copy rather than
"correct".

## AI study (2026-08-18) — tier 14 unblocked, critic-verified

Run at the user's explicit request, ahead of the mission Track A gate the corridor assumed.
Three parallel read-only lanes, then a three-lane critic pass (2026-08-18) that re-proved every
applied row from raw bytes without the applier's reasoning. **Everything below is post-critic.
The first version of this section overstated its headline and got nine numbers or framings wrong;
the corrections are recorded inline rather than silently patched.**

**Applied:** `TEventClass` (88 B), `TActionClass` (148 B), `TagClass` (56 B), `TagTypeClass`
(164 B) created; `ScriptTypeClass.Steps_ActionArgPairs`, `TaskForceClass.Members_CountTypePairs`,
`TeamClass.RecruitedTallyPerTaskForceSlot`, `ScriptClass.CurrentStepIndex` named from proof; a
large evidence plate comment on `TeamClass::AI` 0x006E9140.

**Critic outcome:** structs 1 refuted of 35 rows (2.9%); script VM 0 refuted of 15 claims;
delivery-bar claim PARTIALLY CONFIRMED. All three under the 10% stop-and-report tripwire.

### The delivery-bar finding, as corrected

The corridor and this program both assumed the trigger system was map-scripting for missions.
**It is not campaign-only: the trigger/tag engine runs in ordinary skirmish, ungated by session
type, and the large majority of stock non-campaign maps drive map ambience through it.**

The load-bearing half is the *ungating*, and it survived three separate attempts to kill it:

- Session type is `g_GameMode` at 0x00a8b238 (campaign 0, network 1-4, skirmish 5), identity
  proven independently at `HouseClass__IsHumanPlayer` 0x0050b6f0.
- `Main_Tick` 0x0055d360-0x0055dedb read start to end: `g_GameMode` is read at exactly four sites
  (0x0055d440, 0x0055d7c2, 0x0055dbcd, 0x0055ddaa), each guarding only the mission-timer display,
  a campaign string block, or network UI. At 0x0055d7c2 the campaign and skirmish arms reconverge;
  at 0x0055dbcd the JNZ lands *on* the call site. 0x0055dc99 `CALL 0x0055afb0` executes on every
  arm. **No session-type branch skips the logic tick.**
- `LogicClass::PerTickUpdate` 0x0055afb0 walks the tag vector 0x008b40cc gated only on count and
  three ScenarioClass flags; `TagClass::ProcessTriggerEvent` 0x006e53a0 gates only on
  `g_IsMapEditor` and its own reentrancy bytes. The population loop in FUN_00684c30 *does* contain
  a `g_GameMode == 0` test, but it guards the campaign camera-start block only — the TagType append
  sits outside it.

**Corrected corpus numbers** (machine-derived, 322 of 325 map files parsed; grammar self-check
consumed all 1397 action and 1402 event lines with zero residue):

| First stated | Actual |
|---|---|
| 154 maps in `multimd.mix` | **153 of 170 readable**; true value is a range 153-156 (3 hash-only entries unextractable by name) |
| 57 in `MULTI.MIX`, 0 empty | 57, 0 empty — exact |
| 260 maps, 1396 trigger lines | **261** maps carry `[Triggers]`, out of **322** map files; **1397** lines |
| action 99 used 8832 times | **8840** |

Three framing errors matter more than the counts. The corpus **silently included 15 campaign maps**
in a claim about multiplayer — non-campaign only is **246 maps, 1137 trigger lines, 7947 action-99**,
so ~10% of the headline was campaign content. Only **133 distinct `[Triggers]` section contents**
back those 261 maps, so it is not 261 distinct datasets. And **"every stock multiplayer map" is
wrong**: 186/210 YR-mounted non-campaign maps (**89%**), 243/307 across all non-campaign (**79%**);
`MULTI.MIX` alone is 59%.

**"A skirmish match is SILENT" was also an overstatement.** Action 99 supplies *map ambience* only —
unit, combat, EVA and music audio are unaffected. The accurate statement is that a match without a
trigger engine is **missing its map ambience** (wind, birds, water, city murmur) on roughly nine in
ten stock maps. That is still inside the delivery bar, because `soundmd.ini` marks these
`Control= random loop all`, so a one-shot tag starts *persistent* ambience — the absence is
continuous, not a missed one-off.

**Still unproven, and the phrasing to avoid:** nobody checked whether action 99 is the *only*
ambient source. Theater ambience, a `[Basic]` ambient key, and `Control=ambient` autoplay were never
searched. Action 99 is *an* ambient source; **do not restate "it IS the map ambient-audio system"**
until that is settled.

Action 99 itself is confirmed from the binary: `TriggerAction__Execute` 0x006dd8b0 dispatches a
145-entry table on `actionID - 1`; index 98 -> 0x006de845, which resolves the waypoint to a
coordinate, looks for an object there, and plays either attached to the object (0x005f6cb0) or at
the coordinate via `VocClass__PlayAtCoord` 0x00750e20 — whose only early-out is a negative sound
index. Only action ids **15, 39, 92, 93** route to the no-op stub.

### Two port-critical facts the critic pass produced

1. **A missing house makes a trigger condition PASS, not fail.** 1076 of 1397 triggers are owned by
   `Americans` and 280 by `Neutral` — houses that need not exist in a given skirmish.
   `TriggerCondition__Evaluate` 0x0071e940 does
   `this = Find_By_Country_Index(); if (this == 0) return TRUE;`. A port that returns false here
   silently kills most map ambience while looking correct.
2. **Event 8 evaluates unconditionally true**, proven from raw bytes rather than the decompiler.
   At 0x0071e950, `LEA EAX,[ECX-0xd]; CMP EAX,0x30; JA` makes event 8 *underflow* out of range into
   the default block; the second dispatch at 0x0071ec8d reads selector bytes at 0x0071f284 where
   offset 7 holds 1, routing to 0x0071ecaf and **skipping** the "raised event must match" gate at
   0x0071ec9a. Event 8 is 593 of 675 event instances on multiplayer maps, so this is the dominant
   path, not an edge case. (Cross-check: event 0x12, which does require a match, has selector 0.)

### Corrections to the tier-14 skeleton

- **`TagClass+0x24` is a `TagTypeClass*`, NOT a `TriggerTypeClass*`** — survived three independent
  attacks. The decisive one is a width mismatch: the chain head is read as a **dword** at `+0xA0`
  (0x006e4f1d plus five accessors), but `TriggerTypeClass+0xA0` is a **byte** (0x00726cb4,
  0x00727494) while `TagTypeClass+0xA0` is a dword (0x006e5b88). The alternative typing would read a
  4-byte pointer starting inside a one-byte INI boolean. So the repeat selector lives at
  **`TagTypeClass+0x9C`** (`= atoi(tok1)` at 0x006e60e1; three-way switch 0x006e5426-0x006e542f).
- **`TriggerTypeClass+0x9C..0x9F` are four INI booleans, but my labels were wrong.** All four
  default to **1**, and a `Disabled` flag defaulting to true would disable every trigger in the
  game. The byte that defaults to 0 is **`+0xA0`**, set to 1 from a separate INI token by the map
  loader (0x00727494). First three read as difficulty flags; **`+0x9F` identity is UNKNOWN**.
- `TriggerTypeClass+0xAC` = **events**, built **push-front** so traversal is REVERSED against INI
  order; `+0xB0` = **actions**, tail-appended so traversal MATCHES INI order. The sticky-satisfied
  bitmask uses the traversal position (`1 << (i & 0x1f)`, counter initialised before the walk at
  0x007264c0), so it inherits the reversal. Observable on any multi-event trigger.
- All four class **sizes re-proved from `operator new` callsites**, not constructor tails — the trap
  that produced this program's BuildingClass error.
- The shipped class names are in the binary (`TAction.CPP` path string at 0x00842BF8), so
  `TActionClass` / `TEventClass` are original names, not coinages.
- `TeamClass` ctor DOES zero `+0x38`, and there is **no memset** anywhere in the chain. But "only
  `+0x5C` and `+0x68` uninitialised" overstates: also never written are `+0x15..+0x17`,
  `+0x21..+0x23`, padding `+0x85..+0x87`, and the upper five bits of byte `+0x14`, which an
  `AND 0xf8` deliberately preserves from the raw heap block.
- `INC [owner+0x566C]` is conditional on `IsBaseDefense` (INI string at 0x0081a7ec), so it is the
  house's base-defense team count. Note the sibling: the same ctor **unconditionally** does
  `TeamTypeClass+0xDC += 1`. Two different "team counts" — easy to conflate in a port.

### Script VM — decoded, 0 of 15 claims refuted

65-entry jump table at 0x006E9F74 (guard `CMP EAX,0x40` / `JA` at 0x006e944b/0x006e9455; 65
little-endian pointers ending 0x006EA077, then eight 0x90 NOPs). Traps a port must carry:

- **Opcode 2 has no handler** — slot 2 is byte-identical to the out-of-range target 0x006e95ab, a
  bare epilogue that writes nothing. The cursor advances only when `TeamClass+0x80` is set, and this
  path never sets it, so the team freezes permanently; the only escape (reform) rewinds to step 0
  and re-hits it. **Frequency: 0 occurrences in stock data** — 458 script steps across 88 `aimd.ini`
  scripts and 257 across 52 `ai.ini` scripts contain no opcode 2, and no stock opcode exceeds 0x40.
  Latent, reachable only via custom or map-embedded scripts. A port should still guard it, because
  the native failure mode is a silently frozen team with no error.
- **Opcode 6 stores `arg - 2` and therefore next executes `arg - 1`** (seek 0x006915A0, then the
  next tick's advance 0x006915B0 adds 1). **Frequency: 10 uses in stock `aimd.ini`, top-12** — this
  is live in ordinary skirmish AI, so reading it as a plain jump-to-`arg` misexecutes real scripts.
- **Opcode 0x33's argument is packed 16/16** (`& 0xffff` low, `>> 0x10` high). The low half indexes
  the anim-type array. The high half's meaning is **UNPROVEN** — it is passed as arg3 to the member
  coord vcall and thence to `AnimClass__Constructor`, but the callee's parameter semantics were not
  read. Do not gloss it as "height".
- **The INI key index is not the stored step index** — `FUN_006918a0` walks keys 0..0x31 but its
  write cursor advances only when `ReadString` returned data, so a numbering gap shifts every later
  step down.
- Step layout: `ScriptTypeClass+0xA4 + i*8` = opcode, `+0xA8 + i*8` = argument, 50 max.
  `TaskForceClass` pairs are **count first** at `+0xA4`, **type second** at `+0xA8`, six pairs —
  the half-order the critic most expected to be backwards, and it is correct.

Twelve opcodes account for ~85% of all stock steps (`0` x136, `49` x54, `58` x49, `46` x45, `54`
x34, `5` x30, `47` x24, `53` x22, `11` x16, `6` x10, `14` x9, `43` x9) — that is the minimum
script-VM subset a port needs.

### Corrected trigger subset for tier 14

- events: **{1, 8, 13, 31, 36, 47, 48, 51, 60}** — exactly these nine across the whole corpus, closed
- actions: **{99, 7, 54, 53, 14, 80, 55, 21, 108, 11}** — the first statement of this set wrongly
  **omitted action 55** (36 uses, rank 7) and included 17 (17 uses, rank 11)
- tag repeat modes: **{0: 1194, 2: 119}** — mode 1 never appears
- plus the missing-house-returns-true rule and event 8's unconditional-true path

### Holes carried forward

Seven `TEventClass`/`TActionClass` field names have a proven offset and width but **no located
consumer** (only a constructor zero-init): TEvent `+0x30`, `+0x54`; TAction `+0x30`, `+0x44`,
`+0x4C`, `+0x50`, `+0x90`. Demoted to `Unknown_0x..` with the attempted proof recorded — offsets are
facts, the names were assertions. `TActionClass+0x6D` is `char[32]` or `char[35]`; 3 slack bytes,
no length-bearing copy located. `TagClass+0x34` reads as `IsDisabled` but its only setter also
enqueues the tag onto a global removal vector, so `IsMarkedForRemoval` remains live. `TagTypeClass`
declares 164 B with four well-evidenced fields still unmapped (ID string `+0x24`, 49-byte name
buffer `+0x64`, self-index dword `+0x98`, vtable block `+0x00..0x0C`).

`TriggerClass+0x40`'s sticky mask is 32-bit against a `1 << (i & 0x1f)` shift, so a trigger type
with more than 32 events aliases bit `i % 32` and silently marks a later event satisfied. Latent;
no stock map approaches 32 events.

### Mislabels found (reported, NOT applied — function-boundary edits need separate authorization)

- `TeamClass__Recruit_Or_Add` 0x006E9380 — spurious, zero xrefs, and it holds the interpreter's real
  dispatch. Correction to the first report: its body is **not nested inside** `TeamClass::AI`; the
  two function records **interleave**, so the bogus record has taken blocks *away from* the
  interpreter.
- `TeamTypeClass__AI` 0x006F1090 — is `Read_INI` (~30 `CCINIClass` reads, zero per-tick behaviour).
- `0x00726E00` (`TriggerTypeClass`) and `0x006E5CA0` (`TagTypeClass`) are **destructors** carrying
  `__Constructor` names — both tail-call `AbstractTypeClass__Destructor`, which a constructor never
  does.
- Duplicate names in one namespace: `TriggerTypeClass__Constructor` on both 0x00726C80 (real) and
  0x00726E00 (the destructor); `ScriptTypeClass__Constructor` on **three** addresses — 0x006916B0,
  0x00691970, 0x00691C00. At least one of each set is wrong; the three-way set is **unaudited**.

## Tier 14 — script-VM subset decoded and applied (2026-08-18, post-critic)

Governing snapshot: `<local>/Documents/ghidra-backups/2026-08-18-pre-aistudy`
(21 files, 244,146,185 bytes, verified MATCH with the program closed). All writes below postdate it.
Single-writer discipline held: three critic readers plus a 13-agent read-only decode all stopped
before the first mutation.

### Applied this pass (each with `save_program` + readback)

| Target | Change | Why |
|---|---|---|
| `CellStruct` | **created** — `short X @0`, `short Y @2` | new type, proof below |
| `TagClass+0x30` | `int nCellSentinel` -> `CellStruct AttachedCellCoord` | **the one refuted row** |
| `TagClass+0x24` | `void*` -> `TagTypeClass *` | headline row, survived three attacks |
| `TEventClass+0x30`, `+0x54` | -> `Unknown_0x30`, `Unknown_0x54` | no consumer located |
| `TActionClass+0x30/0x44/0x4C/0x50/0x90` | -> offset-based `pParam_*` / `nParam_*` names | per-action generic block |
| `TActionClass+0x6D` | -> `aParamString2_SizeUnproven` | 3 slack bytes, `char[32]` vs `char[35]` |
| `TagTypeClass` | **added** `aID char[48]` @0x24, `aName char[49]` @0x64, `nSelfIndex` @0x98 | ctor-proven |
| `TriggerTypeClass+0xAC/+0xB0` | renamed to encode insertion direction | the asymmetry IS the finding |
| `TriggerTypeClass+0x9C..0xA0` | **added** five flag bytes, offset-named | defaults proven, semantics not |
| `TeamClass::AI` 0x006E9140 | plate rewritten — opcode table, freeze analysis, two corrections | |
| `TriggerAction__Execute` 0x006DD8B0 | new plate — dispatch, per-action fields, two condition traps | |

Every struct size held after mutation (TagClass 56, TEventClass 88, TActionClass 148,
TagTypeClass 164, TriggerTypeClass 180). The retype-clears-name defect did **not** fire this pass,
but the type-then-name order was used anyway.

**`CellStruct` proof** (my known direction bug class, so it was walked explicitly):
`MapClass::Get_CellClass` 0x005657A0 does `MOVSX EAX,word ptr [EDX+0x2]` then `SHL EAX,0x9`, and
`MOVSX ESI,word ptr [EDX]` — so **+0x2 is Y** (it takes the 512 stride) and **+0x0 is X**, and both
halves are **signed** (`MOVSX`, not `MOVZX`). `TagClass+0x30` is loaded whole from global
0x00B0E700, which is written as two int16 words (0x006E4CF2, 0x006E4CF8), and its sole reader
0x006E52F4 hands it to that accessor at 0x004D8BFF. No instruction anywhere compares the field
against a sentinel — the sentinel-shaped compare at 0x006E54B2/0x006E54C0 tests a *stack* coord
against the *global*, which is what the original metadata conflated.

**`TagTypeClass` ctor proof** (0x006E5B60, read start to end): `MOV [EBP+0x98],0xFFFFFFFF` at
0x006E5B78 then `MOV [EBP+0x98],EAX` at 0x006E5BE6 where `EAX = [0x00B0E790]` — the array count
*before* the increment at 0x006E5C2D, and the object is stored at exactly that index in
[0x00B0E784] at 0x006E5C3A. So `+0x98` is the self-index. `+0x24` is bounded by a `PUSH 0x30`
copy at 0x006E5BBA (48 bytes); `+0x64` is written by `MOV ECX,0xC; REP MOVSD; MOVSB` at
0x006E5BDC = **49 bytes**, defaulting Name to ID (later overwritten from INI at 0x006E610F).
The ctor also registers the object into **two** global arrays, not one: [0x00B0E784] and
[0x00B0F674].

### Script-VM subset — 12 opcodes decoded, covering 95.6% of stock steps

Derived the work list from data first: a section-restricted parse of the 88 sections under
`[ScriptTypes]` in `ini/aimd.ini` (458 steps; a flat parse conflates TaskForce `N=count,TYPE`
lines and is wrong). Twelve opcodes are 438 of the 458.

**Freeze analysis is the load-bearing output** — a handler that never sets `TeamClass+0x80` leaves
the team re-running its step forever:

- **op 0 (136 uses) is terminal BY DESIGN.** A ground team holding a live target can never advance.
  A port that advances here desyncs the script. The two stores at 0x006ED1A4/0x006ED1E1 are
  compiler duplication of *one* decision, not two independent ones.
- **op 11 (16 uses) never advances at all** — no `+0x80` store in the handler or anywhere in its
  callee 0x006ED7E0-0x006EDA8C. 15 of its 16 stock uses are the last step of their script.
- **op 54 (34 uses) is the highest freeze risk.** Its callee contains no `+0x80` store anywhere
  (swept all seven base-register encodings), and all three of op 53's degenerate-path escapes are
  absent. Because the picker runs only on the first-tick flag, a team landing in the
  `+0x40 == 0 && +0x3C == 0` case never re-picks.
- op 43, 14, 47 carry bounded freeze risks; op 6 and 49 advance unconditionally.

**Frequency clauses** (the severity ENGINE.md requires, derived from stock INI):
- **opcode 2's permanent stall: 0 occurrences in stock data.** 458 steps across 88 `aimd.ini`
  scripts, 257 across 52 `ai.ini` scripts, no opcode 2 and none above 0x40. Latent — reachable
  only via custom or map-embedded scripts. Still worth guarding: the native failure mode is a
  silently frozen team with no error.
- **opcode 6's `arg - 2` seek: 10 stock uses, top-12.** Live in ordinary skirmish AI, so reading it
  as a plain jump-to-`arg` misexecutes real stock scripts by one step.
- op 0's argument decoder collapses arg 1, arg 8, arg 0 and arg >= 12 all to mask 0 — **42 of 136
  stock uses (31%)**. Correct behaviour ("no threat filter"), not a decode error.

### Three findings that outlive the typing

1. **Aircraft silently drop mission orders.** Vtable slot `+0x1E8` is the shared mission setter
   0x005B35E0 for UnitClass and InfantryClass, but **AircraftClass overrides it** with 0x0041BA90,
   which is a *filter*, not a forwarder — it can reach `POP ESI; RET 0x8` at 0x0041BADE without
   calling anything. Opcode 11 deliberately admits aircraft (0x006ED8F7), so stock "11,11" steps
   can hand Area Guard to an aircraft whose order is then dropped. Same exposure in op 14, op 5's
   regroup, and every `+0x1E8` call in the shared tails.
2. **Opcode 58's high-index arguments resolve one position early.** The lo16 -> BuildingTypeClass
   mapping is 0-based registry order, confirmed by twenty-plus script-name anchors and flawless up
   to index 67. But every stock use with lo >= 300 lands exactly one short — lo=308 gives an Allied
   wall where index 309 is the Gattling Cannon; lo=357 gives Allied Robot Control where 358 is the
   Yuri refinery. A uniform +1 makes all eight semantically perfect. **Cause unproven** (retail
   authoring bug / in-repo INI drift / registry order diverging above some index). Port guidance is
   identical under every candidate: index in load order and reproduce whatever falls out — do NOT
   "fix" it. 8 of 49 uses resolve to types the owning house can never own, which makes the step a
   silent immediate no-op.
3. **A float constant that a formula gets wrong.** 0x007E2820 *is* pi/2 exactly
   (0x3FF921FB54442D18), but 0x007E2810 is **not** -2*pi/65536: the raw bytes decode to
   -9.58767e-05, about +30 ppm off. Copy the eight raw bytes, never the formula. Reached from
   opcodes 46/47/58 modes 2-3 and the 53/54 bearings, which use x87 doubles with
   sqrt/ftol/atan2/sin/cos — a fixed-point port needs a deliberate decision here.

### A boundary dispute where BOTH agents were wrong

One critic said the bogus record at 0x006E9380 **interleaves** with `TeamClass__AI`; a later agent
said it is **entirely nested inside** it. Settled directly: `TeamClass__AI` body is
0x006E9140-0x006E9F70, `TeamClass__Recruit_Or_Add` is 0x006E9380-0x006E9F51, and probes at
0x006E945B and 0x006E9600 return the bogus record while 0x006E9F55 and 0x006E9F60 return AI.
So it is neither alternating nor simple nesting: the bogus record carves **one contiguous middle
span** out of AI, which keeps the head (0x006E9140-0x006E937F) and a 31-byte tail
(0x006E9F52-0x006E9F70). The dispatch, the table JMP and all twelve handler blocks fall inside the
bogus record, so every xref from the dispatch is misattributed. Comments go on 0x006E9140.

### Not decoded, and honestly bounded

Opcodes present in stock `aimd.ini` but not decoded: 8 (6 uses), 9 (3), 21 (1), 55 (3), 57 (1),
61 (1), 62 (1), 63 (4) — 20 of 458 steps. The trigger-side event and action handlers
(9 events, 10 actions) are specified but not yet individually typed; that is the remaining
tier-14 work.

## Tier 14 CLOSED — trigger subset decoded and applied (2026-08-18)

12 read-only agents (8 events, 9 actions, 8 residual opcodes) plus an adversarial synthesis.
25 items returned, 22 `proven` and 3 `partial`. Applied serially by the sole writer afterwards,
`save_program` + readback after every mutation. Governing snapshot unchanged:
`<local>/Documents/ghidra-backups/2026-08-18-pre-aistudy`.

Mid-pass the Ghidra process crashed outright (no process, port 8089 closed). Relaunched via
`launch-ghidra-mcp.ps1`, reconnected over TCP, and **every prior write was intact** — the
save-after-every-mutation rule turned a crash into a non-event. Worth keeping.

### The (id-1) trap I flagged was a FALSE ALARM — but the real trap is worse

I warned the workers that the action table is indexed on `actionID - 1` and an off-by-one would
silently produce plausible-but-wrong handlers. The synthesizer dumped the raw table
(`read_memory 0x006DFDEC,240` and `0x006DFF20,128`) and checked **every** claimed slot; all ten
were correct and no report applied the bias inconsistently.

The actual hazard is that **this subsystem uses three different index biases**:
- trigger **actions**: `index = id - 1`, bound `CMP 0x90 / JA`, table 0x006DFDEC (145 slots)
- trigger **events**: `index = id - 13`, bound `CMP 0x30 / JA`, selector 0x0071F248, targets
  0x0071F218 — and the default block has its own second dispatch with bias −1
- script **opcodes**: no bias at all, bound `CMP 0x40 / JA`, table 0x006E9F74 (65 slots)

### Applied

`TActionClass` field names are now grounded in the parser **`TActionClass::Read` 0x006DD5B0**,
which decodes the 8-token INI group `ActionID, ParamType, Param3, p4, p5, p6, p7, WaypointCode`.
That parser closed most of the holes the earlier critic forced me to demote:
`+0x30` -> `TeamTypeClass* pParamTeamType` (ParamType 1/5), `+0x4C` -> `TagTypeClass* pParamTagType`
(PT 3), `+0x50` -> `TriggerTypeClass* pParamTriggerType` (PT 2), `+0x44` -> `nWaypointIndex`
(−1 sentinel), `+0x90` -> `nParamScalar_perAction` (zeroed at Read entry, so a wrong ParamType
yields 0 rather than garbage), `+0x48` -> `nToken8_whenParamType0xB`.
**`+0x6D` is settled at `char[32]`** — the earlier "32 or 35, 3 slack bytes" hole is closed by
`strncpy(...,0x1F)` plus `*(char*)(this+0x8C) = 0`, so it spans 0x6D..0x8C inclusive, exactly 32.
Renamed `aCsfLabel`. Still holes with no proven consumer: `+0x34`, `+0x38`, `+0x3C`, `+0x40`,
`+0x54`. Size held at 148.

Three plates written: `TriggerCondition__Evaluate` 0x0071E940 (new, full event decode),
`TriggerAction__Execute` 0x006DD8B0 (rewritten with the field map and action decode), and a
pre-comment on the opcode jump table 0x006E9F74 carrying the residual-opcode decode.

### Findings that change how a port must behave

1. **`a5` is not a scratch out-flag — it aliases the caller's own `a4`.** In
   `EvaluateConditions` 0x007264C0, `0x00726515 LEA EDX,[ESP+0x24]` passes the address of arg4,
   which `ProcessTriggerEvent` set to `(TagClass+0x24)->+0x9C == 2` (repeat-forever). That same
   byte gates the persistence latch at 0x00726582 and the timer re-arm at 0x0072659A. Event 1
   writes it (`0x0071F1FF MOV byte ptr [ECX],1`). **So one successful event 1 latches persistence
   bits for events evaluated after it AND restarts every event-13/51 timer on that trigger,
   regardless of the tag's repeat mode.** Highest-value finding in the corridor.
2. **The raised-event gate is not universal.** Ids 19-22 skip it by explicit compare, id 0 and
   ids >= 60 skip it via the range check, and every id whose L2 selector byte is 1 skips it —
   ids 5, 8-17, 27, 28, 30, 32, 36, 37, 45, 46, 47, 51, 52, 55-58. "Default block implies gate"
   is wrong for all of those.
3. **Per-tick raising is FIRST-TRUE-WINS, not a sweep.** `LogicClass::PerTickUpdate`
   0x0055AFF0-0x0055B16F follows every raise with `TEST AL,AL / JNZ 0x0055B15E`, abandoning the
   chain for that tag. Two groups are further gated on `Scenario+0x34AA` / `+0x34AB`.
4. **The event loop never short-circuits, and an already-latched event counts as TRUE.**
   0x00726547's false path jumps to the LOOP ADVANCE, not the exit; 0x0072650D
   `TEST EBP,EAX / JNZ` skips a latched event and treats it as satisfied.
5. **Action 53 draws RNG; action 54 does not.** Enable-trigger re-arms timers, which draws
   `Rand(0,N)` for any id-51 event on those triggers. Disable-trigger just clears a byte. They are
   deliberately not mirror images — lockstep-relevant.
6. **Event 51's RNG draw happens at ARM time, not evaluation time**, from `[0x00A8B230]+0x218`,
   as `(N/2 truncated toward zero + Rand(0,N)) * 15`.
7. **Action 11's message duration is wall-clock**, `trunc(MessageDelay * 900)` — a port must not
   tie it to the sim tick.
8. **Action 21 (play EVA) has no house gate** — every client hears it.
9. **Opcode 9 (DEPLOY) is an unconditional structural freeze**: any UnitClass member whose type has
   `DeploysInto != 0` blocks the script permanently until it deploys, dies, or is removed.
   Opcode 8 (UNLOAD) freezes on a transport with no legal exit cell.
10. **Opcode 63 can never succeed for vehicles or aircraft**, confirmed byte for byte at
    0x004DFD74-0x004DFD84: the docker argument is computed as `(What_Am_I() == 0xF) ? this : NULL`
    and `CanDock(NULL)` returns 0. The distance test at 0x004DFD6A also runs *before* the CanDock
    call.
11. **Event 36's out-of-range index reads a live stack pointer.** `FUN_00689A00` bounds the local
    variable index to a signed 0..99; outside that it leaves the out byte unwritten, so event 36
    returns effectively TRUE and its complement event 37 returns FALSE.
12. **Events 31 and 48 are unconditionally true** given the raise — the `+0x34` house lookup runs
    and its result is discarded, both branches reaching the same `MOV AL,1`. Together with event 8
    that is three pure acknowledgements a port must not model as polled world-state queries.

### Holes deliberately left open

- **`TriggerTypeClass+0xA4` and its `+0xB4` are undecoded and sit on the hot path** of every
  trigger evaluation (0x00726526 / 0x0072652D / `CALL 0x00502D30` resolves the trigger's own
  house through them). Nobody decoded either.
- **"15 frames = 1 second" is UNPROVEN.** Ids 13/51 arm at `param * 15` and id 47 divides by 15,
  but two consumers of one magic constant do not establish the tick rate. UNCHECKED.
- **Opcodes 55 and 57's brownout-stall decode is single-source and PLAUSIBLE, not proven.** The
  claim rests on which value `FCOMPP` has in ST0, which was never re-read. Marked UNVERIFIED in
  the table comment; do not port the freeze behaviour on it.
- Events 14, 27, 28, 45, 46, 61 have proven handler addresses and undecoded behaviour. None
  appears in the stock corpus.
- `stockUses: 0` on 24 of 25 rows means **UNMEASURED**, not zero. Only action 11 was actually
  counted (19 in the mix packs, 473 campaign, 0 in the 51 loose retail skirmish sections).

### Mislabel batch — now SEVEN, still awaiting authorization

New this pass: **`0x007265C0` is labelled `TriggerActionEntry__PlayVoiceForObjects` and is
actually `TriggerClass::Spring`** — it plays no voice; it tests `+0x44` and `+0x30`, walks the
action list at `[[EDI+0x24]+0xB0]`, and calls `TriggerAction__Execute` per entry. It is the sole
caller of 0x006DD8B0, so the wrong name would propagate into any Rust provenance line citing that
path. Confirmed by two independent workers plus the synthesizer.

Carried over: `TeamClass__Recruit_Or_Add` 0x006E9380 (spurious, zero xrefs, carves the dispatch out
of `TeamClass::AI`); `TeamTypeClass__AI` 0x006F1090 (is `Read_INI`); `0x00726E00` and `0x006E5CA0`
(destructors carrying `__Constructor` names); duplicate `TriggerTypeClass__Constructor` on
0x00726C80 and 0x00726E00; `ScriptTypeClass__Constructor` on three addresses (0x006916B0,
0x00691970, 0x00691C00 — the three-way set is unaudited).

## Residuals and notes

(Accretes across tiers: unproven fields, derived-class spillover, tool defects.)

- 2026-08-17: `search_functions_enhanced` name_pattern filter returns 0 for names the
  plain `search_functions` finds — derive per-class counts live with the plain search.
- 2026-08-17: **MissionClass baseline applied and saved by Codex thread `01a00f6b`**
  (same day): corrected 33-value `/YR_Mission` enum (stale `/RA2/Mission` left
  untouched — its values are wrong for live YR), three mission fields retyped
  (`CurrentMission`/`SuspendedMission`/`QueuedMission` at `0xAC/0xB0/0xB4`), all seven
  base prototypes applied as `__thiscall` with `MissionClass*` receiver, namespace
  association last, saved with two independent zero-mismatch audits. Preserve all of
  it. `Override_Mission`'s two pointer params remain `void *` pending `AbstractClass`.
- 2026-08-17: **research-doc error found by that thread's read-only pass** — the
  MissionClass research report states a 36-byte mission extension; the binary proves
  `0x28` = 40 bytes (`ObjectClass` = `0xAC`, `MissionClass` = `0xD4`, `RadioClass`
  begins at `0xD4`). Also: the stale label `0xB8 = IsCommenced` is wrong — live code
  uses it as a one-byte readiness bypass. Correct the doc when next touching mission
  research; do not cite its size or that name.
- 2026-08-17: **tool defect** — the struct field-type edit tool cleared field names
  while retyping (caught and transactionally repaired by the Codex thread). Every
  future tier must re-read field names after any retype, not just types/offsets.
- 2026-08-17 (tier 5): that defect **reproduced twice** — retyping `AbstractFlags`
  int→byte and `NextObject` ptr→`ObjectClass *` each cleared the name to `(unnamed)`.
  Both caught by the mandatory readback and repaired with `modify_struct_field
  field_name:"offset:N"`. Treat it as certain, not occasional. Note also that the
  create/modify tools auto-prefix names by type (`b`/`n`/`f`/`p`/`dw`), so the applied
  name will not match the requested string — read back and record the actual name.
- 2026-08-18 (AI study): **`search_byte_patterns`'s `mask` parameter silently returns ZERO
  matches for patterns that match exactly when unmasked.** Verified twice: `89 b7 d4 05 00 00`
  matches at 0x006EA56E unmasked, but `89 b0 d4 05 00 00` with mask `FF C0 FF FF FF FF` returns
  nothing; likewise a masked search missed the real `89 86 90 11 00 00` at 0x0066FF8D.
  **This is the THIRD search mechanism in this program whose zero-result is meaningless**, after
  function-scoped `search_instructions` and program-wide operand sweeps. Masked byte searches
  must be treated as non-results. Only an ENUMERATED set of exact byte patterns, or a
  `get_assembly_context` range walk, can support a negative claim. Any hole in this program
  justified by a masked search is downgraded to "not found by an unreliable method".
- 2026-08-18 (final critic pass): **a constructor's last write is a LOWER BOUND, not the class
  size** — and that is exactly how the BuildingClass error got in. The stale 1820 = 0x71C is
  precisely where `BuildingClass__Constructor`'s last write ends (`param_1[0x1c6]` at 0x718, +4).
  Prefer a `Size_Of` stub or an `operator new` site whenever the class has one.
- 2026-08-18 (final critic pass): **abstract base classes have no `Size_Of` stub and no
  `operator new` site**, because they are never instantiated — byte searches for their size
  literals return nothing, which is expected rather than suspicious. The valid method is the
  **derived-constructor boundary**: MSVC emits the base ctor call first, then the derived class's
  own first member at exactly the base size. All four inherited sizes (ObjectClass 172,
  MissionClass 212, TechnoClass 1312, FootClass 1728) were verified this way in the final pass.
  FootClass is the strongest case because the member at the boundary is a **secondary vtable
  pointer**, and a vptr cannot have padding in front of it.
- 2026-08-18 (final critic pass): three cited proof addresses pointed at **function entries
  rather than the instruction**, and one described a 2-byte `6A xx` push as if it were the 5-byte
  `68 xx xx xx xx` form. Values were right, citations would not reproduce. Re-read the
  instruction at the address before writing it into a record.
- 2026-08-17 (tier 11): **an unsigned range check does NOT close a value set.** The applier read
  `CMP EAX,0x6; JA default; JMP [EAX*4+table]` as proving the Jumpjet state field can only hold
  0..6. It does not: `JA` is unsigned, and the default target is byte-identical to table entry 6,
  so 0xFFFFFFFF dispatches exactly like state 6. A jump table bounds the **dispatch outcomes**,
  never the **stored values**. Port consequence: model the extra states as "everything else
  behaves as the last entry", not as unreachable.
- 2026-08-17 (tier 11): **an RTTI Complete Object Locator's `offset` field gives the subobject
  displacement, NOT the class size.** The applier cited COL 0x007FB310's `offset = 0x6C0` as
  corroborating AircraftClass's 1752-byte size; it actually locates a secondary-base vtable at
  object+0x6C0. The size rests on the `operator new` sites alone (five of them, not four).
- 2026-08-17 (tier 11): **a disp32 byte search proves absence only within the ranges you
  classify.** "0x6CA has no writer anywhere" was earned only inside the AircraftClass code range;
  20 hits existed program-wide and 18 went unclassified. A write through a shifted base register
  is invisible to the search regardless. Scope every such negative to the range actually walked.
- 2026-08-17 (tier 10): **`search_instructions` function-scoping UNDER-REPORTS — a zero-match
  result is NOT evidence of absence.** A critic proved two false negatives against itself:
  `mnemonic="stos"` scoped to `BuildingClass__Constructor` returned 0 matches across 334
  instructions while `STOSD.REP` demonstrably exists at 0x0043BA70 and 0x0043BA7D (Ghidra renders
  the mnemonic as `STOSD.REP`, so the substring never matched); and a scoped search on
  `BuildingClass__GetPowerOutput` scanned only 36 instructions, stopping around 0x0044E830 and
  missing the `CALL 0x005F5C60` / `FIMUL` at 0x0044E861-0x0044E866, because Ghidra's function
  body boundary for that symbol ends early. Separately, `operand_pattern` strips `+`, so
  `"ESI + 0x5"` normalizes to `"esi   0x5"` and matches nothing.
  **Consequence for this program: every HOLE justified by "a scoped search returned no matches"
  is weaker than recorded.** Prefer `get_assembly_context` range walks or byte-pattern searches
  on the disp32 encoding when asserting that a field has no reader. The holes already recorded
  in tiers 5-9 on that basis should be treated as "no reader found by a method now known to
  under-report", not as proven absence.
- 2026-08-17 (tier 9): **an imported struct SIZE can be wrong by multiples, and every offset
  above it is then unrepresentable.** `/RA2/HouseClass` carried 22,500 bytes; the binary
  allocates **0x160B8 = 90,296** at three independent sites (0x0068817F, 0x006C3EBC,
  0x0059A17B), each `PUSH 0x160b8; CALL 0x007C8E17; MOV ECX,EAX; CALL 0x004F54A0`. The
  constructor itself writes `[EBP+0x16054]` and `LEA ESI,[EBP+0x16064]`, so at 22,500 it would
  have scribbled ~68 KB past every house. **Check the allocation size before trusting any
  imported class size** — `resize_struct` with `preserve_fields=true` grows non-destructively.
- 2026-08-17 (tier 9): **agent citation addresses drift by a few bytes into mid-instruction.**
  Four of six cited writer addresses were exactly +3 off, landing inside 7-byte disp32 forms and
  returning "No instruction at address". The behavioural claims were all correct. Re-derive the
  address before quoting it in a doc; a wrong address contaminates every downstream reader.
- 2026-08-17 (tier 8): **`remove_struct_field` SHRINKS the structure — it does not clear the
  bytes to undefined.** Removing two 4-byte fields from `/InfantryClass` dropped the size from
  1776 to 1768 and shifted every later field down 8 bytes (Fear 0x6D4 -> 0x6CC, IsProne
  0x6DB -> 0x6D3, WaterSequenceState 0x6E8 -> 0x6E0). Caught immediately by the mandatory
  readback. Recovery that works: `recreate_struct` with `force=true` and the full corrected
  field list, then read back and confirm the size. **Never use `remove_struct_field` to make
  room for a wider type at the same offset** — go straight to `recreate_struct`.
- 2026-08-17 (tier 5): **verify the current name from live Ghidra before every rename.**
  An investigator reported `0x00410410` as `AbstractClass__Save`; it was already
  `AbstractClass__ComputeCRC`. The rename was applied on that false premise and reverted
  immediately (net zero change), but a rename is destructive and agent reports are leads.
- 2026-08-17 (tier 5): **critic false negatives are real.** The struct critic refuted three
  rows (`Dirty` 0x20, `BombVisible` 0x68, `EstimatedHealth` 0x70) for "no reader found";
  all three have readers, missed because its sweeps were mnemonic-filtered to `cmp`/`test`
  and the readers are `mov`/`sub` forms. A critic's negative claim needs the same
  adjudication as a positive one — re-read before acting on a refutation.
- 2026-08-17 (tier 5): the research-doc field table in [OBJECTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_GHIDRA_REPORT.md) was
  wrong in four places (0x3C/0x50 are 16-byte sound handles, not 20-byte `CDTimerClass`
  timers; 0x78 is not `Layer`; 0x84 is not `HasParachute`; 0x8F is not `IsABomb`). Corrected
  in that doc's tier 5 section. Expect the same class of error in the remaining tiers'
  source docs — re-prove, never transcribe.
- 2026-08-17 (tier 5): the **`Reveal`/`Conceal` naming was drift**, not convention. Slots
  +0xD4/+0xD8 are named `Limbo`/`Unlimbo` in TechnoClass and BuildingClass; ObjectClass's
  bases are now consistent. If other classes carry `Reveal`/`Conceal` on those slots, they
  are the same drift.
