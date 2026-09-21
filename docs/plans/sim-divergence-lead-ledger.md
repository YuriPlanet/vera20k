# VERA20k sim divergence lead ledger (FROZEN)

Produced 2026-08-27 by a twelve-lane sweep comparing every VERA `sim/` system group against a
**reference tree**: an external C++ reconstruction of the predecessor engine (Tiberian Sun
2.03), read locally and deliberately not named or linked here. Frozen on completion: see
[Frozen scope](#frozen-scope).

The reference tree is a *lead generator*, not a source. It is cited nowhere outside this
document, and no code, comment, test or commit in this repository derives from it — the rules
below say why, and they are load-bearing rather than decorative.

---

## READ THIS BEFORE USING THE LEDGER

**1. The bar is `gamemd.exe` via Ghidra. The reference tree is a triage lens, nothing more.**

- The reference tree is a reconstruction of a *different, earlier game*. It is not a
  specification, not an authority, and not evidence about Yuri's Revenge.
- **Divergence from the reference tree is a LEAD** — a named place to point a decompile pass. It is not
  a defect, not a verdict, and not a work item until gamemd says so.
- **Agreement with the reference tree verifies NOTHING.** Two reconstructions agreeing is two
  reconstructions agreeing. Every "MATCH-UNCHECKED" row below means only "low-yield place to
  look next", never "correct".
- **No fix, verdict, commit message, test name, or provenance comment ever cites the reference tree.**
  Provenance names the verified native class, function, and Ghidra address, per `AGENTS.md`.
  A lead that survives verification is written up from the *binary*, and the reference reading
  is discarded at that moment.
- Where a lead's own text says "the reference tree does X", read it as "the TS ancestor did X" — never
  as "gamemd does X".

**2. Every lead passes a TS-reachability gate before it is actioned.**

Before any Rust is written from a lead, confirm the behavior is live in an ordinary Yuri's
Revenge skirmish. TS legacy is the most frequent error class from decompilation and the most
frequent error class in this document.

Known dormant or absent in YR — **never implement from a TS reading**: fog of war
(`FogOfWar` defaults false, shroud only), subterranean locomotion, Tiberium veins / veinhole
monsters / weeds, ion storms as a weather system, dropships, drop pods, the Firestorm wall,
rail/train bridges, shroud regrowth (`ShroudGrow=no`), and most `SpecialFlags`-gated
features. `TubeClass` low-bridge movement *is* active and is not subterranean locomotion.

Entries carrying **TS-RISK: yes** are ranked below their raw player-impact deliberately:
they may evaporate entirely under gamemd verification. They stay listed because the check is
still cheap and the answer is still worth having.

**3. A finding exists only in one of two forms.**

- **(a)** A landed fix plus a gamemd-derived test that pins it, or
- **(b)** A residual line carrying its trigger, its player effect, and its frequency.

There is no third state. **Prose never upgrades a status.** Editing an entry's wording,
adding confidence language, or describing a lead more vividly does not move it toward
VERIFIED. Only a decompile citation or an executable check does, and the entry's disposition
field is the only place that is recorded.

**4. Nothing in this document starts verified.** Every entry below is LEAD,
MATCH-UNCHECKED, TS-DIVERGENCE, or NOT-HOMOLOGOUS at freeze time. If you find a row marked
VERIFIED-DRIFT, someone did the Ghidra work and wrote the citation in — check that the
citation is there before trusting it.

---

## Disposition key

| Disposition | Meaning | Actionable? |
|---|---|---|
| **LEAD** | A concrete named difference, both sides cited at file:line. A place to point a decompile pass. Carries no claim about gamemd. | Only after a Ghidra pass and the TS-reachability gate. |
| **MATCH-UNCHECKED** | VERA and the reference tree agree on the mechanism. **Verifies nothing.** Recorded so a later pass does not re-walk it, and to mark it low-yield. | No. Never cite as evidence of correctness. |
| **VERIFIED-DRIFT** | A difference confirmed against `gamemd.exe` by decompile citation or executable check. **Must carry the address inline.** | Yes — this is real work, and it is written up from the binary. |
| **TS-DIVERGENCE** | the reference behavior is TS-only or dormant in YR. VERA's difference is expected. **Closed — do not implement.** | No. Reopening requires binary evidence that YR revived it. |
| **NOT-HOMOLOGOUS** | No reference counterpart exists — YR-only content, or VERA-internal architecture. The reference tree cannot inform it at all. | No. Any question here goes straight to Ghidra. |

---

## Ranking method

Entries are ranked **across all twelve groups**, not by each mapper's self-assigned tier.
The order is:

1. **Player-visibility × frequency.** What an ordinary skirmish player notices, often.
2. **Determinism risk.** Anything that could move replay or lockstep outcomes — RNG draw
   counts, intra-tick ordering, command ordering, authoritative position state.
3. **Internal-only differences and edge cases.**

TS-RISK: yes demotes an entry one tier. One entry (T1-01) is pinned to the top of Tier 1 by
standing instruction: it is an already-reported, reproduced, player-facing bug.

**Counts:** 83 leads (Tier 1: 28 · Tier 2: 38 · Tier 3: 17), 12 MATCH-UNCHECKED group rows,
14 NOT-HOMOLOGOUS rows, 6 TS-DIVERGENCE rows. All twelve mapper sections were present; no
group came back UNMAPPED.

VERA paths are repo-relative. Reference paths are relative to the reference tree root.

---

# TIER 1 — ordinary skirmish, high frequency

#### T1-01 · C4 on a bridge repair hut does nothing — `bridge`
**VERA** the collapse route exists end-to-end and looks correctly wired: `Command::PlantC4`
admits the hut (`src/sim/world/world_commands.rs:1733`), `apply_c4_damage_to_building`
(`src/sim/world/world_orders.rs:911`) detects `bridge_repair_hut` and reroutes to
`dispatch_bridge_collapse_from_hut_with_overlay_registry`, `Immune=yes` is bypassed because
the C4 path sets `ignore_defenses: true`. The one gate that can silently swallow it is the
**claim** condition — the plant fires only once the attacker's own cell is inside the hut
footprint (`world_orders.rs:576-590`).
**Reference** two hut-specific gates, both live for CABHUT: `BuildingClass::Take_Damage`
(`building.cpp:2257`) returns `RESULT_NONE` for `IsBridgeRepairHut && IsImmune` with **no**
`forced` exemption (the adjacent `IsLaserFence` case *does* carry `&& !forced`), and
`InfantryClass::Can_Enter_Cell` (`infantry.cpp:1832`) / `UnitClass::Can_Enter_Cell`
(`unit.cpp:4008`) return `MOVE_NO` for an enemy hut where every other enemy building falls
through to `MOVE_DESTROYABLE`. The reference tree has **no** hut-death → span-collapse route anywhere.
**Disposition** LEAD (proposed mechanism REFUTED 2026-08-27 — see below) · **TS-RISK** yes
**Effect** exactly the reported symptom: a commando bridge cut does nothing.
**Frequency** every attempted commando bridge cut.

**GHIDRA PASS 2026-08-27 — two halves of this entry are now settled, and the entry's own
proposed explanation is refuted.**

*Field identity.* `BridgeRepairHut=` is read at `BuildingTypeClass::ReadINI 0x00460E86`
into `BuildingTypeClass + 0x16B6`. All twelve readers of that offset were enumerated by
instruction search; the four that matter are cited below.

1. **Detonation tail — VERIFIED MATCH, no drift.** `BuildingClass::Update 0x0043FB20`
   C4-expiry tail: a 5×5 scan tests each cell's `+0x44` tile index against the low-bridge
   band `[0x4A..0x65]`; a hit calls low collapse `0x00574C20`, otherwise high collapse
   `MapClass::DestroyBridge_High_OnHutDeath 0x00574000`. Immediately after **either**, at
   `0x00440320`: `MOV byte [ESI+0x6df],0` / `MOV dword [ESI+0x540],0` / `JMP 0x0044035E` —
   marker and timer cleared, damage path skipped. **There is no `TakeDamage` on the hut.**
   The hut survives at full HP and the span dies. VERA's `apply_c4_damage_to_building`
   (`killed_building:false`, `consumed_pending_marker:true`) reproduces this exactly.
   Note the Ghidra label `..._OnHutDeath` names the *trigger's origin*, not the hut's fate —
   it misreads as "the hut dies". It does not.

2. **The `MOVE_NO` hypothesis above is REFUTED as the cause.** The gate is real and survives
   in YR: `InfantryClass::Can_Enter_Cell` reads the flag at `0x0051C62F` and, when set,
   `JNZ 0x0051C7D0` → `MOV EAX,0x7` / `RET 0x14`, an unconditional early return.
   `UnitClass::Can_Enter_Cell` carries the twin read at `0x0073FC00`. Value 7 = `MOVE_NO`
   (the reference tree `astar.cpp:138-147` gives the enum order; the sibling arms triangulate it —
   `0x0051C646` sets 5 = `MOVE_DESTROYABLE` and `0x0051C61F` sets 6 = `MOVE_TEMP`, matching
   the reference tree's `Can_Enter_Cell` arms one-for-one).
   **But it does not block the plant.** `MOVE_NO` governs *pathfinding*, not scripted
   building entry: YR's own `InfantryClass::PerCellProcess 0x00519B7C` reads the same flag
   with the infantry already standing on the hut cell (`CMP EAX,6` = RTTI_BUILDING at
   `0x00519B73`, then the flag test, then the bridge-repair branch), exactly as the reference tree's
   `infantry.cpp:773-804` operates on `Cell_Building()` at `PositionCell`. VERA reaches the
   same state by setting `bypass_grid` on the final one-cell entry move
   (`world_orders.rs:819-828`). So the walk-into-footprint-then-claim sequence is
   structurally faithful and is **not** the bug.

**What remains genuinely open on this entry** — split out as T1-01a below rather than left
buried here: whether VERA implements the `MOVE_NO` refusal *at all*. `bridge_repair_hut` has
only four consumers in VERA (INI parse, audio event, C4 dispatch, engineer repair) and none
is a passability or A* cost test. In YR a hut is never pathable and never a
destroy-through candidate; if VERA prices it as an ordinary destroyable building, an
attack-moving force can select the hut as a path obstacle and cut the bridge by accident.
**Ghidra next:** confirm the A* cost/refusal consumer of `Can_Enter_Cell`'s return in YR,
then check VERA's passability for a hut cell. The original "click does nothing" symptom is
now unexplained by any verified mechanism and should be re-observed in game before more
binary work — the detonation half was already fixed, so the report may predate the fix.

#### T1-02 · Authoritative build duration drops `BuildSpeed` and the minute→frame term — `production`
**VERA** `build_step_time` (`src/sim/production/factory.rs:400`) opens with
`cost × build_time_bonus_ppm`, and the sole production callsite hardcodes that to 1.0
(`factory.rs:993`, "no rules field backs it yet"); the ×0.9 term is recorded as REFUTED at
`factory.rs:390`. Net total ≈ `cost × BuildTimeMultiplier`. Separately
`production_tech.rs:394` computes `trunc(cost × BuildSpeed × 0.9) × BuildTimeMultiplier` and
*that* is the sidebar's remaining-time basis (`production_queue.rs:819`).
**Reference** `techtype.cpp:285` — `Cost * Rule->BuildSpeedBias * (TICKS_PER_MINUTE/1000.)`;
`TICKS_PER_MINUTE/1000` is exactly 0.9 at 15 fps. Stock `RULESMD.INI:41` reads
`BuildSpeed=.7 ; …time (in minutes) to produce a 1000 credit cost item` — 1000 × 0.7 × 0.9 =
630 frames = 0.7 min, matching the key's own documented meaning. VERA's authoritative path
yields ~1000 frames for the same item.
**Disposition** LEAD · **TS-RISK** no (`BuildSpeed` is a live stock-YR `[General]` key and is
already parsed into `rules.production.build_speed_x1000`)
**Effect** everything a player builds takes roughly 1.6× the retail duration, and the cameo
countdown finishes well before the item appears.
**Frequency** constant — every item, every game, from the first Power Plant.
**Ghidra** Decompile YR's `TechnoTypeClass::Time_To_Build` and confirm which of the two VERA
formulas it matches, specifically whether the `BuildSpeed` and 900/1000 terms are both
present. This is one function read and it settles the whole entry.

#### T1-03 · A rearmed aircraft relaunches into Idle and immediately re-enters RTB — `aircraft`
**VERA** `src/sim/aircraft/mod.rs:577-585` releases the dock on the reload tick and moves to
`Docking{sub_state:3}` (Launching); `:604-614` sets the mission to `Idle` once cruising. The
Idle handler calls `enter_idle_mode`, which for an `AirportBound` aircraft with
`is_airborne == true` (`idle_mode.rs:83-86`) returns `ReturnToBase` — sending it straight
back down. `DockedIdle`, the only parked state, is set exclusively at production time
(`production/production_queue.rs:590`).
**Reference** reload completion is owned by the **pad**: `building.cpp:5746-5755` sees
`RADIO_PREPARED == RADIO_ROGER`, assigns `MISSION_GUARD` and calls `Enter_Idle_Mode()`, whose
`HeightAGL <= Landing_Altitude()` branch (`aircraft.cpp:1747`, `:1764-1771`) merely clears
NavCom/TarCom and assigns Guard — the aircraft sits on the pad. The RTB selection at
`:1823-1843` is reachable only from the `In_Air()` branch.
**Disposition** LEAD · **TS-RISK** no
**Effect** a rearmed Harrier or Black Eagle lifts off, flies straight back down, lands, and
repeats indefinitely instead of resting on the pad.
**Frequency** constant — `ORCA` and `BEAG` carry `Ammo=1`, so every sortie ends in a rearm.
**Ghidra** Decompile YR's `AircraftClass::Enter_Idle_Mode` and read the altitude-branch arm:
does the grounded branch assign Guard and stop, and who owns the post-reload mission
assignment — the aircraft or the pad?

#### T1-04 · Blast rocker is parsed but has no producer — nothing in the game ever rocks — `combat` + `aircraft`
*(Merged: `combat` LEAD 1 and `aircraft` LEAD 1 are the same finding, reported independently
by two mappers.)*
**VERA** `rules/warhead_type.rs:101` parses `Rocker=` and `:104` `DirectRocker=`; a
repo-wide search finds **zero** consumers of either in `sim/`, `app/` or `render/`.
`sim/rocking/impulse.rs:51` `apply_rocker_impulse` has no non-test caller. `entity.rocking =
Some(..)` appears only in `rocking/self_destruct.rs:87` and test fixtures, so
`rocking::tick` (`rocking_system.rs:187`) skips every entity at `:198`.
**Reference** `combat.cpp:416-442` — after damage, `rocking_force = min(strength*0.01, 4.0)`;
if the warhead `IsRocker` and force > 0.3, it sweeps a **7×7 cell box** and calls `Rock()` on
every techno. Impact-cell occupiers are rocked away from the *source* with a 10-lepton
displacement; everything else away from the impact coord. (Note VERA's own constant comment
at `rocking_system.rs:52-55` describes a 3×3 loop, which matches neither side.)
**Disposition** LEAD · **TS-RISK** no (`Rocker=`, `DirectRocker=`, `RockerScale=` are stock
YR keys; `RockerScale` is already parsed at `rules/projectile_type.rs:127`)
**Effect** tanks and ships never lurch when a shell, missile or Demo Truck goes off beside
them. The static, unreactive look of a firefight is the symptom.
**Frequency** constant — 18 stock warheads in `ini/rulesmd.ini` carry `Rocker=yes`.
**Ghidra** Find YR's area-damage rocker loop: confirm the force formula, the cap, the
0.3 threshold, and the actual cell-box radius (3×3 vs 7×7). T3-06 holds the kernel-detail
questions and should be answered in the same pass.

#### T1-05 · Enqueue demands the item's full cost; the native gate is one step's worth — `production`
**VERA** `production_queue.rs:187-190` — `if obj.cost <= 0 || owner_credits < obj.cost {
return false; }`, on every click including additions to a non-empty queue tail, silently
rejected.
**Reference** queueing has **no** money check at all (`factory.cpp:251-254`, gated only on queue
cap and build limit). The only money gate is `FactoryClass::Start` (`factory.cpp:376`):
`House->Available_Money() >= Cost_Per_Tick()` — one instalment (`Balance / (54 - stage)`).
Even on failure `Start` has already cleared `IsSuspended` and set the rate
(`factory.cpp:374-375`), so production creeps forward as credits arrive.
**Disposition** LEAD · **TS-RISK** no
**Effect** with 900 credits a player can queue five Grizzlies in retail; in VERA the second
click does nothing. "Build while broke" queuing and partial-progress-while-poor are both
absent.
**Frequency** constant — tight credits are the normal state through most of a skirmish.
**Ghidra** Decompile YR's queue-add and factory-start paths: is there any affordability test
on the add, and is the start-side test one instalment or the full cost?

#### T1-06 · A multi-cell building takes splash once per foundation cell — `combat`
**VERA** `combat/combat_aoe.rs:477-563` walks each spread cell and pushes every occupant;
`AoEDamageResult::push_entity` (`:231`) does no de-duplication, and structures are registered
in **all** their foundation cells (`sim/occupancy.rs:5`, `:1327`). The receiver distance is
keyed to the *scanned cell's* centre (`:944-955`), not the building's own. A 3×3 building
inside a `CellSpread=2` blast produces up to nine damage records for one detonation. No test
in `combat_aoe.rs` uses a multi-cell foundation fixture.
**Reference** `combat.cpp:365-377` builds the object list with `objects.Delete(object);
objects.Add(object)` — an explicit move-to-end de-duplication, so each object appears exactly
once. `:394-406` uses one distance per object: `0` if it is the impact cell's occupier,
otherwise `Explosion_Distance` to the building's own `Target_Coord()`.
**Disposition** LEAD · **TS-RISK** no
**Effect** splash weapons crack large structures far faster than a 1×1 building of the same
HP. If gamemd de-duplicates, artillery/V3/Demo-Truck base-cracking is badly mis-tuned.
**Frequency** every splash hit on any building larger than 1×1 — most base attacks.
**Ghidra** Decompile YR's `Explosion_Damage` blast-list construction and check for a
de-duplicating add, then read which coordinate the per-object distance is taken against.
Note VERA's inline comments already claim the per-cell keying is gamemd-derived — this entry
exists because the magnitude is large and there is no test pin; treat the existing claim as
unverified per `AGENTS.md`.

#### T1-07 · Miner per-gate lift takes the whole remaining capacity, not one density level — `miner`
**VERA** `handle_harvest` passes `empty = capacity_bales - cargo.len()` (up to 40) as the
reduction amount (`src/sim/miner/miner_system.rs:1051-1067`). In `reduce_tiberium`
(`src/sim/tiberium/mod.rs:423-437,481`) an amount ≥ `data+1` takes the full-clear path and
returns the whole pre-removal density, so one 19-frame load gate can erase an entire cell and
add up to 11 bales.
**Reference** `unit.cpp:3033-3038` — `reducer = std::min(1, Capacity - Storage.Total())` then
`Reduce_Tiberium(reducer)`: at most one density level per gate, so a density-11 cell takes 11
gates. (the reference tree's own comment at `:3026-3030` contradicts its `min(1, …)`, so **neither side
is trustworthy here** — read gamemd's actual reducer argument.)
**Disposition** LEAD · **TS-RISK** no
**Effect** how fast a miner fills, how fast an ore field visibly disappears under it, and
whether cells vanish whole or step down through their density frames.
**Frequency** constant — every load gate of every miner.
**Ghidra** Decompile YR's harvest-lift gate and read the literal argument passed to
`Reduce_Tiberium` — a one-line answer that also settles the ore-field visual.

#### T1-08 · Miner unload drains a whole resource slot per dump gate — `miner`
**VERA** at each dump-gate crossing (`unload_tick_interval` = 15 frames) the entire ore slot
drains in one atomic step, credited once (`src/sim/miner/miner_dock_sequence.rs:1204-1240`);
a full miner empties in about two gates.
**Reference** `unit.cpp:3259-3271` — `Storage.Decrease_Amount(1, slot)` inside the same
`HarvesterDumpRate × TICKS_PER_MINUTE` gate: one unit per gate, `Set_Stage(0)`, repeat, so
pad dwell scales with the load carried.
**Disposition** LEAD · **TS-RISK** no
**Effect** how long a miner sits on the pad and whether the credit counter jumps once or
ticks up over several seconds.
**Frequency** constant — every dock cycle.
**Ghidra** Read YR's harvester unload gate: per-gate decrement amount, and whether the credit
grant is per-unit or per-slot.

#### T1-09 · The second path-smoothing pass is a tautology and does nothing — `pathfinding`
**VERA** `pathfinding/path_smooth.rs:392` `optimize_path` — its own doc comment
(`:368-391`) records that `find_drift_segment` compares a quantity with itself, so the
threshold never fires and the function returns its input unchanged. It is still wired into
production (`sim/movement/movement_path.rs:463`, `:520`), so the pass runs and no-ops.
**Reference** `astar.cpp:1203` `Optimize_Moves` is a real pass, called at `:502` right after
`Cut_Corners`. It scans the first 20 moves, tracks a displacement envelope and a Chebyshev
high-water mark, and when the mark stops advancing calls `Splice_Path` (`:1358`) and
`Plot_Straight_Line` (`:1415`). The tail compaction (`:1329-1343`) removes the resulting
holes, so `path->Length` actually **shrinks**.
**Disposition** LEAD · **TS-RISK** no
**Effect** units follow longer, visibly wandering routes — S-curves and detours that retail
replots as a straight run once the obstacle is passed.
**Frequency** constant — every successful A*, and every unit repaths in 24-step segments.
**Ghidra** VERA already cites the YR counterpart at `0x0042B7F0`. Decompile it and recover
the real drift test — the two quantities being compared are the whole answer.

#### T1-10 · Blocked-destination retarget ignores zone, height, and the mover's position — `pathfinding`
**VERA** `sim/movement/movement_path.rs:141-151` `nearest_move_goal` delegates to
`pathfinding/core.rs:2082` `nearest_walkable_any_layer`: expanding square rings returning the
**first** cell walkable on any layer and not entity-blocked. Fixed geometric scan order, no
movement-zone filter, no height filter, no bias toward the mover.
**Reference** the retarget happens **before** the search: `foot.cpp:470-481` replaces the
destination with `Map.Nearby_Location(...)` when the target cell holds a building and the
entry test says `MOVE_NO`. `map.cpp:3658` collects up to 24 candidates across expanding
rings, each filtered by `Is_Clear_To_Move(..., zone, mzone, ...)` — the mover's own movement
zone — and by `|height − candidate height| < 2`, prefers on-screen, and returns the candidate
**nearest the mover's current cell** (`:3777-3829`).
**Disposition** LEAD · **TS-RISK** no
**Effect** retail stages the unit on the side facing it and never picks an unreachable cell;
VERA can pick the far side of a structure, or a cell across a cliff or water in a
disconnected zone, after which the follow-up path fails and the unit refuses the order.
**Frequency** every move or attack order whose target cell is blocked — right-clicking a
building, attack-moving into a base, force-firing a structure. Several times a minute.
**Ghidra** Locate YR's `Nearby_Location` equivalent and read three things: whether it filters
by the mover's movement zone, whether it carries the `< 2` height window, and what it sorts
on.

#### T1-11 · Crusher speed clamp while driving over a victim is absent — `movement`
**VERA** `update_vehicle_speed_fraction` has three arms only (slowdown-brake, accelerate,
decelerate) — no crush arm. Already written down as a known deferral at
`src/sim/movement/drive_locomotion.rs:201-212`; implementation at `:253-293`.
**Reference** `drive.cpp:967-970` — inside `While_Moving`'s `IsAccelerates` branch,
`if (LinkedTo->IsCrushing) { TargetSpeed = min(TargetSpeed, 0.2); Set_Speed(TargetSpeed); }`,
pre-empting both the accelerate and decelerate arms. `IsCrushing` is raised on entering a
cell with a crushable overlay (`drive.cpp:1372-1377`) and cleared in per-cell processing.
**Disposition** LEAD · **TS-RISK** no (crushing and `Crusher=` are core RA2/YR)
**Effect** an Apocalypse / Ore Miner / MCV rolling over infantry or a fence keeps full speed;
retail visibly bogs to a fifth speed for the crush.
**Frequency** every engagement in which a crusher meets infantry or a wall — VERA's own note
counts 15 stock `Crusher=yes` types that also accelerate, including `[APOC]`, both MCVs,
`[V3]` and every ore miner.
**Ghidra** Decompile YR's drive-locomotor speed ramp and look for a crush arm that pre-empts
the accel/decel branches; recover the literal clamp value.

#### T1-12 · Retaliation has no out-of-range gate for human-owned units — `combat`
**VERA** `combat/combat_targeting.rs:606` `should_retaliate_from_damage` gates on
`can_retaliate`, alliance, mind-control, `MissionControl Retaliate=`, an existing TarCom for
human houses, and an AI threat score — but contains **no** distance term. `tick_retaliation`
(`:720`) then assigns `AttackTarget::new(attacker_sid)` unconditionally (`:798`) and marks it
`passively_acquired_target = false` (`:800`), exempting it from the scanner's
release-on-range-loss path.
**Reference** `techno.cpp:5153-5165` — after `Is_Allowed_To_Retaliate` passes it computes
`retaliate = In_Range(target, which)`. If not in weapon range, a **human-player-owned** object
retaliates only when `Distance(target->Center_Coord()) <= (SightRange + 0.5) * CELL_LEPTON`;
an AI-owned object retaliates unconditionally.
**Disposition** LEAD · **TS-RISK** no
**Effect** a player's tanks break formation and chase an out-of-sight attacker (V3,
Dreadnought, sniper) across the map instead of holding position.
**Frequency** every engagement involving a longer-ranged attacker; every time siege units
shell a defended position.
**Ghidra** Decompile YR's `TechnoClass::Take_Damage` retaliation tail and read whether the
out-of-range arm carries a human-vs-AI split and a `SightRange`-derived distance bound.

#### T1-13 · A damaged unit's smoke system does not follow the unit — `particles`
**VERA** `src/sim/particles/spawn.rs:195-249` creates the damage smoke system at the entity's
position plus `DamageSmokeOffset` and stores `owner_entity`. `smoke.rs:29-113` never re-reads
that owner — it spawns from the frozen `sys.coords` (`:90-94`). A grep across `src/sim/`
finds no reader that repositions a system.
**Reference** `partsys.cpp:429-431`, the first statement of `Smoke_AI`:
`if (Source->As_ObjectClass() != NULL && Source->What_Am_I() != RTTI_BUILDING)
Set_Coord(Source->Center_Coord() + CoordOffset);` — the system rides the source every frame
with the constructor-captured offset; buildings alone are exempt.
**Disposition** LEAD · **TS-RISK** no
**Effect** a damaged tank drives away and leaves a static clump of smoke puffs at the map
spot where it first went yellow, instead of trailing smoke.
**Frequency** constant — 177 sections in `ini/RULESMD.INI` carry `DamageParticleSystems=`.
**Ghidra** Decompile YR's smoke particle-system AI and read its first statement: does it
re-anchor to the source object each frame, and is there a building exemption?

#### T1-14 · Smoke and gas particles never move — `particles`
**VERA** `smoke.rs:118-127` runs state advance, lifetime countdown and deceleration only.
`move_smoke` (`smoke.rs:136`) and `move_gas` (`gas.rs:134`) are `#[allow(dead_code)]` with no
production caller; `smoke_wind_dir()` (`smoke.rs:156`) hardcodes north. Deceleration floors
at zero (`smoke.rs:124-126`).
**Reference** `particle.cpp:723-750` `Smoke_Motion_AI` moves every puff each frame — wind step
`SmokeWindX/Y[Rule->WindDirection]` every `10/WindEffect` frames, then on odd frames the gas
velocity, a downward settle capped at 2 leptons, the accumulated drift, and a clamp keeping
it 5 leptons above ground. `particle.cpp:759-786` does the rising equivalent for gas.
Deceleration is floored at **3.0**, not 0 (`particle.cpp:431-433`). The per-tick random drift
that feeds it (`:399-421`) is absent from VERA too. `[General] WindDirection=1` is present in
retail at `ini/RULESMD.INI:399`.
**Disposition** LEAD · **TS-RISK** no
**Effect** smoke and gas hang as a motionless cluster of sprites at the spawn point instead
of rising, wandering and blowing downwind. Every plume in the game reads as static.
**Frequency** constant — damaged units and buildings, refinery vents, building smokestacks.
**Ghidra** Decompile YR's smoke and gas particle motion routines: recover the wind table
indexing, the settle/rise steps, and the deceleration floor.

#### T1-15 · The refinery's dock message is a bare ack; in the ancestor it is the whole choreography — `docking`
**VERA** `radio/receive.rs:117-121` — `RadioMessage::CanDock` returns `CellAccepted` and
performs **no state writes**. Approach, turn and pad entry are driven entirely from the miner
side (`sim/miner/miner_dock_sequence.rs`), acknowledged at `radio/receive.rs:156-159`.
**Reference** `building.cpp:454-557` — the same message runs, in order: a power gate (`:457`); a
**building-initiated** `RADIO_HELLO` back to the docker when the building holds no link
(`:507`); a `needs_to_move` comparison of the docker's NavCom against `Docking_Coord()`
(`:513-519`); a `RADIO_NEED_TO_MOVE` poll (`:530`); `RADIO_MOVE_HERE` carrying the pad cell
`Get_Cell() + Cell(2,1)` (`:533-538`); and, when the docker answers "already there",
`RADIO_TETHER` then `RADIO_BACKUP_NOW`, with `from->Scatter(...)` if the docker refuses
(`:544-547`). The docker re-sends every mission tick (`foot.cpp:2306`), making it a
continuous engine-side correction loop.
**Disposition** LEAD · **TS-RISK** no for the loop; **yes** for the `!IsOn` power sub-gate
(YR refineries are widely believed to accept ore unpowered — verify that branch separately)
**Effect** the building never re-steers a docker that drifted off the pad cell and never
ejects one that cannot back in; in the ancestor a stuck harvester is scattered and the bay
frees itself.
**Frequency** constant — the message fires every tick for every miner in a dock cycle.
**Ghidra** Decompile YR's `BuildingClass` handler for the docking opcode and enumerate its
arms — the correction loop's existence is the question, and T3-05 (opcode identity) gates
which case number to read.

#### T1-16 · Path segment is regenerated one step too late — the curve is lost at every 24-cell boundary — `movement`
**VERA** `handle_path_exhaustion` returns `NotExhausted` while `target.next_index <
target.path.len()` (`src/sim/movement/movement_tick.rs:417`), so the re-path runs only after
the last step is consumed. Meanwhile the curve chooser reads `path[next_index + 1]` and
returns `None` at the final step (`movement_step.rs:59-66`), normalised to "straight".
Segment cap `MAX_PATH_SEGMENT_STEPS = 24` (`pathfinding/core.rs:114`).
**Reference** `drive.cpp:1848-1863` — `Start_Of_Move` reads `nextface = Path[1]`, and when that
is empty *and* the destination is still more than `2 * CELL_LEPTON` away it calls
`Basic_Path` right there, re-reads `Path[1]`, and selects the track with a real next facing.
Path buffer is `CONQUER_PATH_MAX = 24` (`sun.h:114`) — the same size.
**Disposition** LEAD · **TS-RISK** no
**Effect** on any move longer than one path segment a vehicle straightens out for the
boundary cell and then turns, instead of carrying the curve; on a sharp boundary turn it can
drop into a stop-and-rotate.
**Frequency** every ordered move longer than 24 cells — cross-map attack moves, miners on
long ore runs.
**Ghidra** Decompile YR's `Start_Of_Move` and check for an inline re-path when the path
lookahead is empty and the destination is still distant.

#### T1-17 · Firing is a separate global phase; in the ancestor it happens inside each object's own turn — `world` *(determinism)*
**VERA** the whole live-object walk (mission dispatch + every locomotor) runs first at
`src/sim/world/mod.rs:6334-6547`. Combat then runs as Phase 5 over a **fresh** order snapshot
taken at `mod.rs:6770`, with `tick_turret_rotation` after it at `:6803`.
**Reference** `unit.cpp:546` calls `Firing_AI()` inside `UnitClass::AI`, after that same
object's `BASECLASS::AI()` (`:504`) has already run its mission dispatch (`mission.cpp:241`)
and its locomotor (`foot.cpp:3281`); `Rotation_AI()` follows at `:555`. Object #1 fires,
kills, and rotates before object #2 has taken any of its turn.
**Disposition** LEAD · **TS-RISK** no
**Effect** native lets a shot kill a unit that has not yet moved this frame — the victim
loses its step. In VERA every object completes its move before any shot resolves, so shots
lead against post-move rather than pre-move positions. At the margin this changes hit/miss
and one cell of death position for the same inputs.
**Frequency** constant — every engagement, every frame with a shot in flight.
**Ghidra** Decompile YR's `UnitClass::AI` and locate the firing call relative to the base-class
call and the locomotor. Re-seating this means re-seating the spine, so verify it together with
T1-18 and T2-11 in one pass.

#### T1-18 · Unit and Infantry get two Ready→Commence checkpoints per tick; the ancestor gates the second on cell arrival — `mission` + `world` *(determinism)*
*(Merged: `mission` LEAD 2 and `world` LEAD 2 are the same finding.)*
**VERA** first checkpoint in the object-AI stage (`mission_common_step`,
`sim/world/techno_ai.rs:511-517`), immediately before Phase-1 ground movement; a second,
`object_ai_post_movement_promote_one` (`techno_ai.rs:179-195`), called **unconditionally**
per object right after its locomotion from `sim/world/mod.rs:6546`, gating only on
`evaluate_ready` (`sim/mission/authority.rs:660`). The doc at `techno_ai.rs:127-149` states
the twice-per-tick claim explicitly but cites addresses only for the Aircraft case.
**Reference** `UnitClass::AI` gates **once**, at `unit.cpp:498-501`, before `BASECLASS::AI()`;
`InfantryClass::AI` likewise at `infantry.cpp:1392-1395` before `:1407`. The only other
promotion is inside `Per_Cell_Process` — `unit.cpp:2295` (`!IsDumping &&
Ready_To_Commence()`) and `infantry.cpp:1010` (an *ungated* `Commence()`) — which runs only
on an actual cell transition, not every frame. Aircraft alone match VERA
(`aircraft.cpp:648`, after `:626`).
**Disposition** LEAD · **TS-RISK** no
**Effect** every mission handoff that depends on having come to rest lands one tick earlier
in VERA, and the intra-tick order of mission state versus movement differs. Visible as
attack-move / waypoint chains stepping one frame ahead per link; chained handoffs compound.
VERA's own comment names harvest dock/unload/exit and guard→attack as affected.
**Frequency** constant — it is the per-tick shape of every unit and infantryman.
**Ghidra** Decompile YR's `UnitClass::AI` and `InfantryClass::AI`, count the
`Ready_To_Commence` callsites, and read `Per_Cell_Process` for the second one — including
whether YR kept the ungated infantry `Commence()` and the `IsDumping` guard.

#### T1-19 · The Guard handler's un-named cadence short-circuit is shaped like the weapon rearm timer — `mission` *(determinism / RNG)*
**VERA** `evaluate_foot_guard_cadence` (`sim/world/techno_ai/mission_handlers.rs:852-863`)
always returns `[Rate] + RandomRanged(0,2)` (via `jittered_mission_cadence` `:930`) except on
the bunker-delegate arm. The doc block at `:838-848` records a deliberately unmodelled
short-circuit: a three-dword object timer at `+0x2EC`/`+0x2F4` that, while live, returns its
own remaining frames and draws **no** RNG; role UNCHECKED, one arming site at `0x007464AB`.
**Reference** `foot.cpp:914` — `return((Arm != 0) ? (int)Arm : (dtime+Random_Pick(0, 2)));`.
`Arm` is `CDTimerClass<FrameTimerClass> Arm` (`techno.h:179`), assigned
`Arm = Rearm_Delay(which)` inside `TechnoClass::Fire_At` (`techno.cpp:3993`). In the ancestor
shape the short-circuit is the weapon reload countdown and the arming site is the firing path.
**Disposition** LEAD · **TS-RISK** no (`Arm`/`Rearm_Delay` and the Guard mission are live YR)
**Effect** a guarding object that has just fired re-enters its Guard handler when the weapon
finishes reloading rather than on `[Guard] Rate` (26 frames stock), and consumes **no**
scenario-RNG draw on that dispatch. VERA draws one every time, so the RNG stream diverges on
top of the cadence.
**Frequency** every engagement — any unit holding Guard or Sticky that shoots.
**Ghidra** Read `0x007464AB` and the object fields `+0x2EC`/`+0x2F4`: is the timer written by
the fire path, and is it the rearm delay? That one identity settles both the cadence and the
draw count.

#### T1-20 · The immediate mission set resets the handler cursor and dispatch timer — `mission`
**VERA** `assign_base` (`sim/mission/verb.rs:26-31`) → `assign_transition`
(`sim/mission/state.rs:139-147`) writes `current`, clears `queued`, clears
`movement_bypass_latch`, and **additionally** sets `handler_state = 0`,
`mission_start_frame = now`, `ai_counter = 0`, `dispatch_timer = at_frame(now)`.
**Reference** `MissionClass::Set_Mission` (`mission.cpp:146-151`) writes exactly three fields —
`CurrentMission`, `MissionQueue = MISSION_NONE`, `IsMissionUnloadStandby = false`. `Status`
and `Timer` are **not** touched. Only `Commence` (`:308-318`) does `Timer = 0; Status = 0;`.
**Disposition** LEAD · **TS-RISK** no
**Effect** in the ancestor shape a retasked object inherits the previous mission's `Status`
cursor (so its new handler can start mid-sequence) and its `Timer` (so it can sit idle for up
to the previous handler's full delay — 30 s on a base stub — before its first dispatch). In
VERA it always starts clean and dispatches immediately.
**Frequency** constant — every non-queued mission set, including map `MISSION=` placement and
every engine-internal retask.
**Ghidra** Decompile YR's `SetMission` and `Commence` and read exactly which fields each
writes. The question is only whether YR moved the reset set up into `SetMission`.

#### T1-21 · Burst state is destroyed when the target dies instead of persisting for one rearm delay — `combat` *(+RNG)*
**VERA** `burst_remaining`, `burst_delay_ticks` and the ROF countdown live on the
`AttackTarget` record (`combat/mod.rs:556-562`). `combat/mod.rs:6029` ("Phase 5: remove
AttackTarget from finished attackers") sets `attack_target = None`, discarding all three; the
next `AttackTarget::new` starts a fresh burst from zero. `retarget_preserving_rearm`
(`combat/mod.rs:950`) preserves the record on a *swing* to a new victim, but is not used when
the target is removed.
**Reference** `techno.cpp:3551` `Assign_Target` — when the target is cleared and
`BurstIndex != 0` it does **not** zero the burst counter: it computes
`BurstResetTimer = Rearm_Delay(which)` with `BurstIndex` temporarily forced to `Burst` (the
full-ROF branch, including its `Random_Pick(0, 2)` draw) and sets `IsBurstResetPending`
(`:3586-3608`). Assigning a *new* target clears the pending reset but leaves `BurstIndex`
alone.
**Disposition** LEAD · **TS-RISK** no (`Burst=`/`BurstDelay%d=` are live YR keys; VERA
already models the mid-burst draw against `TechnoClass::GetROF @ 0x006FCFA0`)
**Effect** a burst weapon whose victim dies mid-burst gets an immediate full fresh burst on
the next target instead of resuming with its remaining count — visibly higher sustained DPS
sweeping through weak units. Secondary: an RNG draw at target-clear that VERA does not
consume, shifting the scenario stream.
**Frequency** every time a burst weapon (Gattling, Flak, IFV, Rocketeer, Destroyer) kills a
target mid-burst — most fights against infantry or light vehicles.
**Ghidra** Decompile YR's `Assign_Target` and read the target-clear arm: is the burst index
preserved behind a timer, and is a draw consumed there?

#### T1-22 · Aircraft descent uses the climb rate; the ancestor's descent is proportional — `aircraft`
**VERA** one constant serves both directions — `movement/locomotor.rs:123`
`FLY_CLIMB_RATE = 300` leptons/second (20 per frame at 15 Hz), applied symmetrically at
`air_movement.rs:494` (ascend) and `:510`/`:529` (descend). No cargo-state distinction.
**Reference** `fly.cpp:632-636` — climb is `min(is_loaded ? 10 : 20, FlightLevel - height)`, so
a loaded transport climbs at **half** rate. `fly.cpp:663-672` — descent is `gap / 20` clamped
into `[20, 50]`: fast at altitude, floored at 20 near the ground. With RA2's `FlightLevel=1500`
(`ini/RULESMD.INI:67`) that is roughly 42 frames to land against VERA's 75. `:688-697` also
nudges a descending aircraft 5 leptons/frame toward its destination cell centre, except when
landing on a helipad under `MISSION_ENTER`.
**Disposition** LEAD · **TS-RISK** no (generic flight parameters; `FlightLevel` is already
read from RA2 rules in VERA, so only the ramp shape is at issue)
**Effect** landings and dive-descents take roughly 2.5× longer, so airfield turnaround
visibly drags; loaded transports climb out too fast.
**Frequency** constant — every takeoff and every landing, plus the dive-bombing altitude
change at `aircraft/mod.rs:334-341`.
**Ghidra** Decompile YR's fly-locomotor altitude step and recover the climb and descent
expressions separately, including any loaded-cargo halving.

#### T1-23 · Shot-down aircraft do not tumble, and three RNG draws are not consumed — `aircraft` *(+RNG)*
**VERA** no production code sets `rocking.is_ship_rocking` or writes
`vel_forwards`/`vel_sideways` (grep outside `sim/rocking/` returns only field declarations in
`components.rs:1084-1090` and hashing in `world_hash.rs:1375-1379`). Aircraft death runs the
ordinary dying/despawn path. `rocking_system.rs:262-264` restricts ship-rocking support to
`EntityCategory::Unit`, explicitly excluding aircraft.
**Reference** `aircraft.cpp:4210-4219` — `Crash()` sets `IsRocking`, `Stun()`s the aircraft, then
draws **three** RNG values: `RockingSidewaysPerFrame = Random_Double(0.10, 0.25)`, a
`Random_Pick(0, 1)` sign flip, and `RockingForwardsPerFrame = Random_Double(0.0, 0.1)`.
`techno.cpp:7957-7961` integrates with **no** clamping and **no** damping while `IsRocking`
holds; `fly.cpp:1318-1332` feeds the angles to the draw matrix. (VERA's
`advance_ship_rocking`, `rocking_system.rs:136-154`, *does* clamp both axes.)
**Disposition** LEAD · **TS-RISK** no
**Effect** a Harrier, Black Eagle or Kirov shot out of the sky drops flat and level instead
of tumbling. Separately, three RNG draws per air kill are not consumed — a lockstep
divergence if gamemd draws from a shared stream.
**Frequency** every AA kill; every engagement in a match with air units.
**Ghidra** Decompile YR's aircraft crash entry and count the RNG draws and their ranges, then
read whether the crash-rocking integration is clamped.

#### T1-24 · Water impacts do not select the splash animation set — `combat`
**VERA** `combat/mod.rs:1259` emits the warhead's `AnimList=` entry with `idx = damage / 25`
clamped to `len - 1` (`:1225`) for every impact regardless of land type.
`rules/warhead_type.rs:98` parses `Conventional=` but nothing in `sim/` reads it, and
`SplashList=` is only registered as an animation-type name in `rules/ini_parser.rs:1105` —
there is no `splash_list` field and no consumer anywhere.
**Reference** `combat.cpp:641-647` — before the `ExplosionSet` branch, if the impact land type
is water, the warhead is `Conventional`, and the impact is not on a bridge deck, the
animation comes from `Rule->SplashList` with a **different** bucket size:
`min(damage, 35 * count - 1) / 35` rather than `/ 25`.
**Disposition** LEAD · **TS-RISK** no (`SplashList=` and `Conventional=` are both authored in
stock `rulesmd.ini`)
**Effect** shots into water show a land explosion instead of a water plume. Stock
`[General] SplashList=H2O_EXP3,H2O_EXP2,H2O_EXP1` never renders.
**Frequency** every naval engagement and every missed or force-fired shot into water —
constant on naval maps.
**Ghidra** Decompile YR's combat-animation selector and read the water/`Conventional`/bridge
gate and the splash bucket divisor.

#### T1-25 · Refinery is reserved on arrival, not at selection time — `miner`
**VERA** the close-radio HELLO path is chrono-only by an explicit early return
(`src/sim/miner/miner_system.rs:1504-1512`); a far War Miner just drives to the staging cell
with no HELLO (`:1646-1685`), and the shared `dock_reservations` admission is taken only on
reaching contact range (`:1288-1296`).
**Reference** FINDHOME transmits `RADIO_HELLO` to the chosen bay **before** moving
(`unit.cpp:3543`), and only a `RADIO_ROGER` advances to HEADINGHOME → `MISSION_ENTER`; a
refusal re-searches ignoring occupancy and, if farther than 3 cells, drives to a cell beside
the busy refinery (`:3546-3567`).
**Disposition** LEAD · **TS-RISK** no
**Effect** with two refineries and two loaded miners, claiming at selection time splits them;
claiming on arrival lets both drive to the same refinery and one then waits.
**Frequency** every return once a player has 2+ miners — constant after the first minutes.
**Ghidra** Decompile YR's harvest FINDHOME state and read whether the bay claim is
transmitted before or after the drive, and what the refusal path does.

#### T1-26 · A fully charged superweapon is never put on hold by low power and stays firable — `superweapon`
**VERA** `src/sim/superweapon/mod.rs:231-256` — the per-instance loop opens with
`if !inst.is_active || inst.is_ready { continue; }`, so the suspend/resume block at `:241-247`
can never run on a charged weapon. A ready instance always reports `is_suspended == false`
(and `is_online == true` to the sidebar, `mod.rs:188`). The launch gate
(`src/sim/world/world_commands.rs:2008-2013`) checks only `is_active && is_ready` —
suspension is not consulted at all.
**Reference** two separate functions. `SuperClass::AI` (`super.cpp:387`) does gate on `!IsReady`,
but suspension is driven from the house power pass at `house.cpp:8725-8735`, which calls
`Suspend(true)`/`Suspend(false)` on every present, powered super weapon **regardless of
readiness**; `SuperClass::Suspend` (`super.cpp:168`) has no `IsReady` test. `Can_Place`
(`super.cpp:582`) returns false whenever `IsSuspended`, and `State_String` (`:570`) reports
the HOLD caption.
**Disposition** LEAD · **TS-RISK** no (all stock YR superweapon sections carry `IsPowered=true`
and HOLD is ordinary YR sidebar behaviour)
**Effect** with the base browned out VERA still lets the player fire a charged Nuke / Iron
Curtain / Weather Storm and the cameo still reads as online.
**Frequency** every power dip while a superweapon sits charged — routine from mid-game on
(plant sniped, overbuilding, an infiltration blackout). VERA's own `force_shield.rs:44-51`
deliberately triggers an owner blackout, so it can produce the state itself.
**Ghidra** Decompile YR's superweapon suspend path and its firing gate: does suspension apply
to a ready weapon, and does the launch admission read the suspended flag?

#### T1-27 · Prerequisites and the factory count ignore a building until its build-up animation ends — `production`
**VERA** the prerequisite scan requires `e.building_up.is_none()`
(`production_tech.rs:284-290`), and so do the MultipleFactory input
(`matching_factory_count_for_owner`, `:606-613`) and the has-a-factory gate (`:333-338`).
**Reference** both counters increment at Unlimbo — at placement, before the animation runs.
`techno.cpp:1673` calls `House->Tracking_Active_Add(this,false)` inside `TechnoClass::Unlimbo`,
incrementing `ABQuantity` (`house.cpp:6195`) — the exact counter `Can_Build` reads
(`house.cpp:927`, `:1031`). `building.cpp:1984` calls `House->Active_Add(this)` in
`BuildingClass::Unlimbo`, incrementing the per-RTTI factory counter (`house.cpp:5102`) that
`Factory_Count` returns (`:5419`).
**Disposition** LEAD · **TS-RISK** no for the counters as substrate; the *timing* of the
gamemd increment still needs the read
**Effect** after placing a Barracks or War Factory the dependent cameos stay greyed and no
production speed-up applies until the build-up animation ends; the ancestor unlocks and
counts the moment the foundation goes down.
**Frequency** per building placed (roughly 15–30 times a skirmish), each for the length of one
build-up animation.
**Ghidra** Find YR's `Unlimbo` for buildings and read where the prerequisite/quantity counter
and the factory counter are incremented relative to the build-up state.

#### T1-28 · Movement budget is not discarded while a vehicle turns in place — `movement` *(determinism)*
**VERA** when `handle_vehicle_rotation` returns `StillRotating` the per-entity loop
`continue`s (`src/sim/movement/movement_tick.rs:1914-1917`) without touching
`drive.residual_budget`; the leftover 0..6 units survive the whole rotation and are spent on
the first frame after it completes. `residual_budget` is cleared only on path exhaustion
(`movement_step.rs:1615`).
**Reference** `drive.cpp:941-944` — `While_Moving`'s first legality gate discards it outright:
`if (… || (IsRotating && !TClass->IsTurretEquipped)) { SpeedAccum = 0; return false; }`, with
the in-source comment "No speed should accumulate if movement is on hold." Note the turret
exemption — a turreted vehicle does not trip that clause.
**Disposition** LEAD · **TS-RISK** no
**Effect** none visible on any single frame (at most one extra track point, well under a
cell), but it is authoritative position state and it compounds across every turn.
**Frequency** constant — every vehicle turn-in-place, which is most direction changes from a
standing start.
**Ghidra** Decompile YR's drive `While_Moving` entry gate and check for the accumulator reset
and the turret exemption.

---

# TIER 2 — regular, but conditional on map, matchup, or terrain

Format is compressed. Every entry is a LEAD unless marked otherwise, and every one still
requires the TS-reachability gate.

#### T2-01 · Units on a collapsing high-bridge deck survive — `bridge`
**VERA** `drop_in_bridge_deck_entities` (`src/sim/world/bridge_orchestrator.rs:1760`) snaps
deck occupants to ground level with health untouched; `kill_ground_occupants_at` (`:1312`)
explicitly filters `!e.is_on_bridge_layer()`. **Reference** `CellClass::Destroy_Bridge`
(`cell.cpp:1417`) kills the ground occupier with a forced C4Warhead hit and calls
`Fall_From_Height()` on the deck occupier; `object.cpp:331` sets `IsFalling`/`IsToExplode` and
the object explodes when it lands.
**TS-RISK** no · **Effect** a column crossing when the span drops lives, and is left standing
at ground level over water — the payoff of the whole "cut the bridge under them" tactic is
gone. · **Frequency** every deliberate bridge cut with traffic on the span.
**Ghidra** Decompile YR's per-cell bridge destruction and read the two occupier fates
(ground kill vs deck fall) separately.

#### T2-02 · No kill for units moving onto a span as it collapses; fallout is per-cell inline, not a deferred batch — `bridge`
**VERA** `blow_up_bridge_cell_fallout` (`bridge_orchestrator.rs:1298`) runs per collapsed cell
inline and touches only that cell; no neighbourhood sweep exists and
`notify_bridge_span_collapse` is a deliberate skirmish no-op. **Reference** `Destroy_Bridge` only
*queues* (`cell.cpp:1435`); `MapClass::Damage_Bridge` (`map.cpp:12041`) runs the family
handler to completion, then loops `On_Bridge_Collapse()` (`map.cpp:12086-12089`), which
sweeps a 5×5 neighbourhood over **both** object lists and kills any foot whose
`Locomotion->Is_Moving_Here(coord)` is true (`cell.cpp:6130`).
**TS-RISK** no · **Effect** a unit that has stepped off the ramp toward the dropping span
keeps its movement and arrives on a destroyed cell instead of dying with it. · **Frequency**
every bridge cut with inbound traffic — most of them.
**Ghidra** Look for a deferred pending-cells queue drained after the family handler in YR's
bridge damage entry, and for an `Is_Moving_Here`-gated neighbourhood kill.

#### T2-03 · Low-bridge damage stages do no occupant-legality kill — `bridge`
**VERA** the low walkers (`src/sim/bridge_state/walker.rs:509` → `destroy_bridge_walker_*_low`)
emit `CellAction::BlowUpBridge` only on the final stage; intermediate stages produce
`StateOutcome::Absorbed` with no occupant effect, and no `Kill_Illegal_Occupiers` analogue
exists anywhere in the tree. **Reference** `Damage_Low_Bridge_EW`/`_NS` and both `_Piece_`
variants call `Recalc_Attributes(-1)` then `Kill_Illegal_Occupiers()` on all three span cells
at **every** stage (`map.cpp:9078-9083`, `:9154-9159`, `:9296-9301`, `:9375-9380`).
`cell.cpp:6204` kills each occupier failing `Can_Enter_Cell` with a forced full-strength
C4Warhead hit, then sweeps 5×5 for feet moving in. Legality-based, so a hover/amphibious
occupant survives — a different mechanism from T2-01.
**TS-RISK** no · **Effect** a tank on a wooden or concrete low bridge over water survives the
collapse and is left standing on water. · **Frequency** every low-bridge destruction; low
bridges are common on stock YR maps.
**Ghidra** Decompile YR's low-bridge stage handlers and check for a per-stage legality kill.

#### T2-04 · Bridge-damage Z window is ground-anchored and unconditional; the ancestor's is deck-anchored and under-bridge-conditional — `bridge`
**VERA** `path_matches_cell` (`src/sim/bridge_state/mod.rs:997-1006`) requires
`impact_z ∈ [level-1, level+1]` — a symmetric 3-level window around the cell's *ground* level,
applied on both state-machine paths with no precondition; direct-overlay paths carry no Z gate
(`mod.rs:945-947`). **Reference** `combat.cpp:481` gates the high-bridge block with
`!cellptr->IsUnderBridge || coord.Z <= BRIDGE_LEPTON_HEIGHT + LEVEL_LEPTON_H*(Height+1) &&
coord.Z > BRIDGE_LEPTON_HEIGHT + LEVEL_LEPTON_H*(Height-2)` — anchored on the **deck**,
half-open, skipped entirely when the cell is not flagged as under a deck. The low-bridge block
(`:526`) has no Z test, matching VERA's ungated direct paths.
**TS-RISK** no (VERA already records a related residual at `src/sim/combat/mod.rs:1472`)
**Effect** a `Wall=yes` blast at ground level under a high span damages the span in VERA and
would be rejected by a deck-anchored window; conversely an air-burst at deck height over an
unflagged cell passes there and can be rejected here. · **Frequency** constant on bridge maps.
**Ghidra** Decompile YR's bridge-damage routing in the explosion path and recover the exact Z
window and its `IsUnderBridge` precondition.

#### T2-05 · The hierarchical corridor gate has no bridge-layer exemption — `pathfinding`
**VERA** `pathfinding/core.rs:1355-1361` applies `HierarchyGate::allows` (`:382-387`) to every
compass neighbour with no test of `neighbor_use_bridge`. VERA's own `zone_search.rs` header §5
states the rule it is aiming at. **Reference** `astar.cpp:412` gates on
`… && base_level && …`, where `base_level` (`:406`) is
`!IsUnderBridge || abs(CurrentCellHeight - Height) <= 1` — the corridor skip is never reached
for a neighbour on a bridge deck at a differing height.
**TS-RISK** no · **Effect** on a hierarchical search across a high bridge, deck cells can be
refused because the corridor was stamped from ground-level zones — the unit declines to cross
or takes a long detour. · **Frequency** every hierarchical path over a high bridge; constant
traffic on bridge maps.
**Ghidra** Decompile YR's A* neighbour gate and look for a bridge/height term guarding the
corridor skip.

#### T2-06 · Blocked-destination near-miss tests the start height, not the expanding node's — `pathfinding`
**VERA** `pathfinding/core.rs:1310` — the exit fires when
`start_height.abs_diff(goal_height) <= 1`, fixed for the whole search. **Reference**
`astar.cpp:467-470` tests `abs(CurrentCellHeight - DestCellHeight) <= 1`, where
`CurrentCellHeight` is refreshed on every pop (`:480-482`, seeded `:291-297`).
**TS-RISK** no · **Effect** ordering a unit onto a blocked cell at a different elevation —
VERA can refuse the order where retail walks adjacent and stops, or accept it where retail
refuses. · **Frequency** every blocked-cell order across a height change; common on plateau
and ramp maps.
**Ghidra** Decompile YR's A* blocked-destination tail and read which height variable feeds the
`<= 1` test.

#### T2-07 · Airfield selection never screens pad availability — `docking`
**VERA** `docking/aircraft_dock.rs:243-294` `find_nearest_airfield` filters on category,
alive, owner, `UnitReload`/`Helipad` and `Dock=` membership, then keeps the minimum
`cell_dist_sq`. Occupancy is not consulted — `AirfieldDocks::has_free_slot` (`:185`) exists
but is not called. **Reference** `techno.cpp:7631-7670` screens every candidate with a live
`Transmit_Message(RADIO_CAN_LOAD, building) == RADIO_ROGER` (`:7659`); the building answers
NEGATIVE when it already holds a different contact (`building.cpp:398`), so a busy pad leaves
the candidate set. `|| building->IsLeader` (`:7667`) overrides distance for the primary.
**TS-RISK** no for the single-pad helipad case; the multi-pad airfield half is YR-only and
the YR selection rule must come from gamemd
**Effect** with two helipads, one occupied, a VERA aircraft flies to the nearer busy one and
loiters. · **Frequency** every engagement once a player runs more aircraft than pads — normal
Rocketeer/Harrier/Black Eagle play.
**Ghidra** Decompile YR's docking-bay search and read whether availability is screened during
the scan and what the primary-building override does.

#### T2-08 · A servicing depot does not hold its client still, and the repair step is ungated — `docking`
**VERA** `docking/building_dock.rs:277-327` — the `Servicing` arm counts down and fires
`repair_tick` with no check on whether the client is moving; movement is cleared once, on
arrival (`:271`). `repair_tick` (`:88-116`) heals `repair_step` and never snaps to full; an
unfunded client is ejected after `NO_FUNDS_GRACE_TICKS = 30`. **Reference**
`building.cpp:5548-5566` calls `radio->Locomotion->Power_Off()` and nulls NavCom while
servicing, powering back on at release (`:5578`, `:5619`); each step is gated on
`Transmit_Message(RADIO_NEED_TO_MOVE) == RADIO_ROGER` (`:5641`); the final step snaps
`Strength = MaxStrength` (`techno.cpp:1089-1094`); on out-of-cash the depot drops to IDLE and
retries with no timed ejection (`building.cpp:5663-5671`).
**TS-RISK** no for the pin and the snap-to-full; **partial** for the cash-out branch, which is
entangled with TS-only `IsUseless` sell-back logic (`building.cpp:5601-5605`) — do not carry
that over
**Effect** three visible differences: a repairing unit can be ordered off the pad mid-repair;
the last step leaves a few HP short; at zero credits VERA drives the unit off after ~2 s.
**Frequency** every repair-depot visit.
**Ghidra** Decompile YR's service-depot repair state and read the three arms: client
lock/unlock, the per-step gate, and the completion write.

#### T2-09 · Aircraft attack approach flies straight at the target; the ancestor picks a standoff cell off a ring — `aircraft`
**VERA** `attack_mission.rs:156-178` (state 3) computes a Chebyshev cell distance and issues
`approach(..., (status.rx, status.ry))` — the target's own cell. State 4 (`:203-223`) keeps
approaching until the ±11.25° arc gate (`FIRING_ARC_TOLERANCE`, `:26`) passes. No standoff, no
per-shot repositioning, no airspace deconfliction; `CurleyShuffle` appears nowhere in `src/`.
**Reference** `Do_MISSION_ATTACK` state `PICK_ATTACK_LOCATION` (`aircraft.cpp:2198`) sets NavCom
to `Good_Fire_Location(TarCom)`, which walks rings inward from `range - 1 cell`, samples 16
facings per ring, filters through `Cell_Seems_Ok` (which inspects other aircraft NavComs to
avoid mid-air collisions, `:2966`), keeps best and second-best, and picks between them with
`Percent_Chance(50)` (`:2933`) — an RNG draw per pick. `FIRE_AT_TARGET2` re-enters the pick
whenever `Rule->IsCurleyShuffle` (`:2380`, `:2397`, `:2410`); RA2 `RULESMD.INI:55` has
`CurleyShuffle=yes`.
**TS-RISK** partial — `Good_Fire_Location`/`Cell_Seems_Ok` are generic and almost certainly
live in YR, but the 5-shot strafe path they branch around (`aircraft.cpp:2162-2166`, `:2417+`)
is **not** reachable for stock YR ground-attack aircraft (`Is_Strafe()` requires bullet
`ROT <= 1`; `Maverick`/`Maverick2` use `AirToGroundMissile` with `ROT=100`). Do not port the
strafe cadence.
**Effect** aircraft bore straight in instead of settling at a standoff on the edge of weapon
range, and multiple aircraft attacking one target do not spread. · **Frequency** every air
attack for the standoff half; the `CurleyShuffle` re-pick is narrower (`Ammo=1` stock).
**Ghidra** Decompile YR's aircraft attack mission and check for a fire-location search, its
ring/facing sampling, its airspace filter, and the RNG draw.

#### T2-10 · Aircraft splash-distance halving is gated on `IsHighFlying`, not on being an aircraft — `combat`
**VERA** `combat/combat_aoe.rs:880` halves the collected 3D distance only when
`is_high_flying(entity)` is true, and only inside the separate airborne collection phase
(`:831`), itself entered only when the impact coordinate is strictly above ground height
(`:793`, `:433`). **Reference** `combat.cpp:403-405` halves the distance for **every**
`RTTI_AIRCRAFT` object in the blast list, unconditionally and in the same loop as ground
objects.
**TS-RISK** no, but VERA's inline comment already claims the `IsHighFlying` vslot gate is
gamemd-verified — the two readings cannot both be the YR rule, so treat the existing claim as
unverified.
**Effect** low-flying aircraft (Rocketeers, hovering Kirovs) take less splash in VERA than
the wider gate would give. · **Frequency** every AA splash engagement and every ground blast
under an aircraft.
**Ghidra** Decompile YR's blast-list distance step and read the aircraft arm's gate condition.

#### T2-11 · Vision is a whole-map per-tick recompute; the ancestor reveals inside each object's turn with cadence gates — `world`
**VERA** `refresh_fog` (`src/sim/world/mod.rs:6675`) calls
`vision::recompute_owner_visibility_in_place`, which clears the visible flag on every owner
grid and re-reveals from every entity, once per frame, after the entire movement pass.
Per-object work inside the walk is limited to `move_unit_sensor_after_cell_change` (`:6516`).
**Reference** reveal is per object inside `Per_Cell_Process`: `unit.cpp:2270` and
`infantry.cpp:1012` call `Look(false)` when `IsPlanningToLook` (set by the locomotors when the
object skipped more than one cell) and `Look(true)` — an *incremental* look — otherwise. Two
cadence gates: `infantry.cpp:1441` re-looks while moving at most once per second via
`LookTimer`, and `aircraft.cpp:659` gates on `SightTimer`.
**TS-RISK** partial — fog of war is dormant in YR, so `Encroach_Fog` (`logic.cpp:305`) and the
`FogRate` path are out of scope; only the shroud-reveal half is portable. Shroud regrowth is
also out of scope (`ShroudGrow=no`, retail `rulesmd.ini:677`).
**Effect** shroud lifts at a different moment inside the frame relative to that frame's
combat, and the incremental-vs-full distinction plus the once-per-second infantry cadence do
not exist, so a fast infantry blob peels shroud on a different schedule. · **Frequency**
constant.
**Ghidra** Verify with T1-17 and T1-18 in one spine pass: where in YR's object turn does the
look happen, and are there incremental/full and timer variants?

#### T2-12 · Lightning Storm does not touch airborne units — `superweapon`
**VERA** `src/sim/superweapon/lightning_storm.rs:59-120` sets owner/target, deferment,
duration and ambient lighting; the tick body generates bolts. Nothing in the module iterates
units. **Reference** `ion.cpp:182-193` — on the frame the storm breaks, `Ion_Storm_Begin` walks
`Feet` backwards, calls `Locomotion->Power_Off()` on every non-limboed ion-sensitive foot, and
calls `Crash(NULL)` on anything `RTTI_AIRCRAFT` (with an index fix-up because crashing
shortens the list). `Ion_Storm_End` (`:262-268`) powers the same set back on.
**TS-RISK** no for the aircraft arm (aircraft crashing during a storm is ordinary RA2/YR);
**yes** for the ion-sensitive **jumpjet** half — jumpjet-as-locomotor is TS content. The
tint/theme-swap/screen-static presentation (`ion.cpp:196-235`) is TS-only.
**Effect** Kirovs, Rocketeers, Harriers and Black Eagles fly through a Weather Storm
untouched. · **Frequency** every match where a storm is fired with aircraft up.
**Ghidra** Decompile YR's Lightning Storm start and read whether it walks the object list and
what it does to aircraft specifically.

#### T2-13 · `RevealOnFire` reveals to the shooter, not to the house being shot at — `vision`
**VERA** `src/sim/combat/mod.rs:7256-7263` pushes
`RevealEvent { owner: snap.owner, …, radius: REVEAL_ON_FIRE_RADIUS }` (radius 3,
`combat/mod.rs:108`) on every shot of a weapon with `reveal_on_fire`; consumed at
`src/sim/world/mod.rs:6918` into the **firer's** own grid. No gate on whether the firer is in
shroud, none on the target's house. **Reference** `techno.cpp:4072-4079` — the reveal happens
only when the firer is undiscovered or on a shrouded/fogged cell (with an aircraft carve-out),
resolves the **target's** owning house, requires that house to be player-controlled, and calls
`Map.Sight_From(Center_Coord(), 2, tgt_owner)` — radius **2**, into the **victim's** map.
**TS-RISK** no for the mechanism (it is in the ordinary `Fire_At` path), but the reference tree has no
`RevealOnFire=` key at all, so YR clearly reworked this — radius, recipient and gates must all
come from gamemd
**Effect** in the reference the victim gets a hole punched around an attacker that opened fire
from the dark — how you find the thing shooting you. In VERA the shooter reveals around
itself, a no-op. · **Frequency** every engagement where a unit fires from unscouted ground;
heaviest in the first minutes and around static defences.
**Ghidra** Decompile YR's `Fire_At` reveal arm and read the recipient house, the radius, and
the gate conditions.

#### T2-14 · `RevealOnFire` default polarity — `vision`
**VERA** `src/rules/weapon_type.rs:194` — `unwrap_or(false)`. A weapon that does not author
the key never reveals. **Reference** no `RevealOnFire` key exists; the `Fire_At` reveal is
unconditional for any weapon. Retail `RULESMD.INI` authors the key 11 times and **every one is
`=no`** (zero `=yes`) — the authoring pattern of a flag whose default is *yes*.
**TS-RISK** no · **Effect** if the gamemd default is yes, no stock weapon in VERA ever
produces the muzzle reveal at all — the mechanism is dead, not merely mis-aimed. ·
**Frequency** constant; it changes nearly every weapon.
**Ghidra** Read the `WeaponTypeClass` INI field-map default for `RevealOnFire` in gamemd.
Cheapest entry in the ledger. **If the default is `yes`, T2-13 and T2-14 both promote to
Tier 1** — the mechanism then fires in every early-game engagement rather than never.

#### T2-15 · `AttackingAircraftSightRange` not implemented — `vision`
**VERA** no consumer; the key appears nowhere under `src/`. Aircraft reveal only through the
ordinary per-tick `reveal_entity_vision` (`vision/mod.rs:1273`). **Reference**
`aircraft.cpp:1148-1157` — for a player-controlled aircraft, if its own position, any of three
cells two away, or the target's cell is shrouded, it calls
`Map.Sight_From(PositionCoord, Rule->AttackingAircraftSightRange, House)`.
**TS-RISK** no — retail `RULESMD.INI:407` carries `AttackingAircraftSightRange=2` with the
authors' own comment about making the V3 "ping the map", so the key is live and its consumer
set may be wider than aircraft
**Effect** an air strike into unexplored territory does not lift a patch of shroud at the
firing point. · **Frequency** every air attack or V3 salvo into unscouted ground.
**Ghidra** Find the gamemd consumer(s) of `AttackingAircraftSightRange` and read the gate.

#### T2-16 · No height-column techno discovery when a cell is first mapped — `vision`
**VERA** `reveal_radius_into` (`vision/mod.rs:1398-1426`) sets cell flags and nothing else.
No per-object discovery pass, and no walk of the other cells projecting to the same screen
position. **Reference** `DisplayClass::Map_Cell` calls `Map.Reveal_Nearby_Technos(cellptr, house,
newlymapped)` on every cell whose mapped state changed (`display.cpp:1123`). That routine
(`map.cpp:11709-11726`) steps the cell by `(1,1)` seven times and at step `i` (i = 1,3,…,13)
reveals the techno there if that cell's `Height` falls in `[i-2, i]`, calling
`t->Revealed(house)` and, when newly mapped, `Map.Radar_Cell(...)`.
**TS-RISK** no for the mechanism; the step count and height window must be re-read from
gamemd. Note `TechnoClass::Revealed` (`techno.cpp:834`) also carries a MISSION_AMBUSH →
MISSION_HUNT side effect that belongs to the mission group and is TS-risky on its own.
**Effect** a unit on a cliff top stays undiscovered until the reveal spiral physically reaches
its own cell, so cliff-top defenders appear later than they should. · **Frequency** every
scouting approach toward high ground — common on the many stock plateau/ramp maps.
**Ghidra** Decompile YR's per-cell map/reveal commit and look for a height-column techno
discovery walk.

#### T2-17 · Allied vision is a live merge of whole grids, not a one-shot permanent write — `vision`
**VERA** `FogState::build_merged_for` (`vision/mod.rs:875-895`) rebuilds the merged view every
tick from the *current* alliance map, OR-ing every friendly owner's entire grid including
`FLAG_REVEALED`. `[General] AllyReveal` is never parsed or consumed. **Reference** two separate,
both permanent, mechanisms: (a) at alliance formation `house.cpp:2166-2174` runs one
`Sight_From` per *live* allied Techno, gated on `Rule->IsAllyReveal`; (b) thereafter
`MapClass::Sight_From` (`map.cpp:1086-1089`) redirects an allied house's sight to `PlayerPtr`.
Nothing is ever un-shared.
**TS-RISK** no — `AllyReveal=yes` is at `RULESMD.INI:751`
**Effect** allying mid-game hands you your new ally's *entire* explored map instantly, and
breaking an alliance re-shrouds everything they had shown you; in the reference shroud never
comes back. A map or mod setting `AllyReveal=no` has no effect. · **Frequency** every mid-game
ally/un-ally in a team game; never in fixed-teams 1v1.
**Ghidra** Decompile YR's alliance-formation handler and the sight-write redirect, and confirm
whether `AllyReveal` is read.

#### T2-18 · Reveal is clipped to the map rectangle, not the playable diamond — `vision`
**VERA** `reveal_radius_into` (`vision/mod.rs:1402`) — the only per-cell admission test is
`rx >= 0 && rx < w && ry >= 0 && ry < h`, a rectangle over the whole fog grid seeded from
`session.map_width`/`map_height` (`sim/world/mod.rs:3021`). **Reference** `Sight_From` tests
`In_Radar(newcell)` per target cell (`map.cpp:1110`) *and* `In_Radar(cell)` on the viewer's
shifted centre before doing anything (`:1054`); `In_Radar` (`:1166-1177`) is the isometric
playable-diamond test against `PlayRect`.
**TS-RISK** no — the playable-rect concept is unchanged in RA2/YR
**Effect** VERA lifts shroud on border cells outside the playable diamond, and an aircraft or
reinforcement that leaves the playable area keeps revealing instead of going blind. VERA does
model the *viewer* side (`vision/mod.rs:1254`); the per-target-cell clip is what is missing. ·
**Frequency** constant at map edges.
**Ghidra** Decompile YR's `Sight_From` and read the per-cell admission test.

#### T2-19 · Terrain speed misses the ≥2-level "treat as road" substitution, and slope is compared in cell levels — `movement`
**VERA** `terrain_speed_factor` always reads the destination cell's own land row
(`src/sim/pathfinding/terrain_speed.rs:216`), and `slope_factor_for` compares `cell_level()`
(`:206`, `:238`). No height-difference override exists in the chain. **Reference**
`drive.cpp:1789-1798` — before the land-row lookup,
`if (abs(cell_height - Map[destcell].Height) < 2) { ground = destcell.Land_Type(); } else
{ height = cell_height; ground = LAND_ROAD; }`, where `cell_height` already carries
`+BRIDGE_CELL_HEIGHT` on a bridge. The uphill/downhill choice (`:1802-1820`) uses
`Get_Height_GL` — lepton ground heights, which differ across a ramp tile inside one level.
The zero→0.5 substitution (`:1823`) and the `ConditionYellow → ×0.75` (`:1826-1828`) both
match VERA.
**TS-RISK** low — the `LAND_ROAD` substitution and GL comparison are ordinary terrain
handling, but `BRIDGE_CELL_HEIGHT` and the bridge model need YR confirmation
**Effect** vehicles entering or leaving a bridge deck take the underlying land row's penalty
instead of road speed, and ramp tiles never trigger the downhill bonus. · **Frequency** every
bridge entry/exit and every ramp crossing — routine on hill and river maps.
**Ghidra** Decompile YR's per-cell speed lookup and read the height-difference branch and the
units of the slope comparison.

#### T2-20 · `Stop_Moving` does not clamp the drive target speed — `movement`
**VERA** `drive_stop_moving` (`src/sim/movement/navcom.rs:253-283`) clears the destination and,
only when there is no committed head-to, zeroes the *applied* fraction — it never clamps the
*target* fraction. VERA's own Ship path *does* clamp (`ship_stop_moving`, `:284-300`), so the
two locomotors disagree with each other inside VERA. **Reference** `drive.cpp:441-462` —
`Stop_Moving` ends with `TargetSpeed = min(TargetSpeed, _deaccel)` where `_deaccel = 0.3`
(`drive.cpp:83`), then `DestinationCoord = COORD_NONE`; the committed head-to is deliberately
left alone, so the unit coasts into its claimed cell at no more than 30% throttle.
**TS-RISK** no · **Effect** how far a vehicle carries past the moment you press Stop. ·
**Frequency** every explicit stop order and every order overridden mid-move — constant in
micro-heavy play.
**Ghidra** Decompile YR's drive `Stop_Moving` and read whether it clamps the target speed and
to what.

#### T2-21 · No per-tick zone-validity abort of an in-flight move — `movement`
**VERA** the zone map is consulted only when a path is searched
(`src/sim/movement/movement_path.rs:269`, `pathfinding/zone_map.rs:120`, used from
`movement/group_destination.rs:135` at order time). The per-entity tick body
(`movement_tick.rs:1494`) has no destination-zone test; a mover whose destination becomes
unreachable keeps blocking and re-pathing until `PATH_STUCK_INIT = 10` retries run out.
**Reference** `drive.cpp:648-654` — every `Process` tick, before starting a new track,
`if (IsLocked && Mission != MISSION_ENTER && Is_Moving() &&
!Is_In_Same_Zone(DestinationCoord)) { Stop_Driver(); Abandon_Navigation(); }`.
**TS-RISK** no for the mechanism — **but note the `IsLocked` gate**: in the reference tree `IsLocked` is
set from `Map.In_Local_Radar(...)` (`techno.cpp:1665`), i.e. **viewport-dependent**, which
would be a determinism hazard here. Verify what gates the equivalent test in gamemd before
porting the gate along with the check.
**Effect** destroy a bridge or wall off a ramp under a crossing column and VERA's units keep
pushing and re-pathing at the far shore; the ancestor drops the order and idles them. ·
**Frequency** whenever ground connectivity changes during an active move — bridge kills,
wall-offs, gate closes.
**Ghidra** Decompile YR's drive `Process` head and look for a same-zone destination test and
what gates it.

#### T2-22 · Refinery candidate set accepts allied buildings and has no reachability gate — `miner`
**VERA** `find_nearest_refinery` accepts any refinery of a *friendly* house
(`src/sim/miner/miner_system.rs:1734-1740`) and ranks purely by squared cell distance to the
dock cell, with no zone/reachability test (`:1762-1770`); the deposit credits the refinery's
owner (`miner_dock_sequence.rs:1240-1252`). **Reference** `Find_Docking_Bay` is called with
`friendly = false` from the harvest path (`unit.cpp:3533`), restricting candidates to
`building->House == House` (`techno.cpp:7654`), and additionally requires the building to be
in the same movement zone as the unit's destination (`:7658`).
**TS-RISK** no · **Effect** two symptoms — a miner can hand its load to an *ally's* refinery,
and on a multi-zone map it can select a refinery it cannot path to and stall. · **Frequency**
allied case, every team game with adjacent bases; unreachable case, per return on
island/plateau maps.
**Ghidra** Decompile YR's harvest bay search and read the ownership argument and any movement
zone filter.

#### T2-23 · Dock admission ignores the building's construction/deconstruction state — `docking`
**VERA** `radio/receive.rs:166-188` `refinery_hello` gates on `dying`, `health.current == 0`
and owner equality, then inserts. Nothing consults the building's mission or build state.
VERA also has no multi-tick deconstruction window at all —
`sim/production/production_sell.rs:725` ejects, interrupts docked miners and uninits in one
call. **Reference** `building.cpp:398` refuses `RADIO_CAN_LOAD` under
`MISSION_CONSTRUCTION || MISSION_DECONSTRUCTION || BSTATE_CONSTRUCTION`, or when the building
already holds a different contact; `RADIO_IM_IN` refuses under deconstruction too (`:435-437`);
`RADIO_ARE_REFINERY` additionally refuses on `IsInLimbo` and on cargo already attached
(`:564-568`).
**TS-RISK** no · **Effect** during a refinery's sell/build animation an inbound miner is
admitted and commits to a dock that is about to vanish. · **Frequency** per building sold or
newly placed — rare, but it lands on the player's economy.
**Ghidra** Decompile YR's refinery/dock admission handler and enumerate the state refusals.

#### T2-24 · The transport unload-standby / movement-bypass latch has no production writer — `mission`
**VERA** the only setter of `movement_bypass_latch` is
`set_movement_bypass_after_verified_queue` (`sim/mission/state.rs:195-198`), which is
`#[cfg(test)]`. The consumer is live: `unit_ready_to_commence` blocks a moving unit unless the
latch is set (`sim/mission/readiness.rs:175`). With no writer the exemption never applies.
**Reference** `IsMissionUnloadStandby = true` is written at `unit.cpp:3166-3168` — the transport
unload machine's `CLOSING_DOOR` state does `Assign_Mission(MISSION_GUARD);
IsMissionUnloadStandby = true;` **in that order**, so the latch survives the queue write. Read
at `unit.cpp:5992` as the last term of the moving-blocks-commence gate; every base transition
clears it, which VERA already mirrors.
**TS-RISK** no · **Effect** a transport that has just dropped passengers while still rolling
cannot commence Guard until it stops; in the ancestor it flips to Guard immediately and starts
scanning sooner. · **Frequency** per transport unload — every APC / IFV / Flak Track / Battle
Fortress drop.
**Ghidra** Decompile YR's transport unload state machine and find the write site of the
standby flag relative to the mission assignment.

#### T2-25 · Build-list side identity comes from a static house country, not a live ConYard's `ActLike` — `production`
**VERA** `owner_matches_build_identity` (`production_tech.rs:195-204`) matches against the
owner name or `house.country`, a fixed property. `ActLike`/`act_like` appears nowhere in
`src/`. **Reference** `house.cpp:1051-1067` — for a building type whose `Get_Ownable()` names
exactly one side, `Can_Build` walks `ConYards` and requires a live one (`!IsInLimbo && IsOn &&
Mission != MISSION_DECONSTRUCTION`) whose `ActLike` is in that mask.
**TS-RISK** no — `ActLike` is live in RA2/YR and the country-vs-ActLike distinction is the
whole basis of captured-ConYard building
**Effect** capturing an enemy Construction Yard should add that side's structures to the
sidebar and losing it should remove them; in VERA the list stays pinned to the starting
country. Same path is why a powered-down or being-sold ConYard withdraws the list. ·
**Frequency** rare per match but decisive — engineer captures are a standard mid-game play and
MCV-swap is a known YR tactic.
**Ghidra** Decompile YR's `Can_Build` side-ownership arm and read whether it walks live
ConYards and what property it tests.

#### T2-26 · Structures can be queued behind an in-progress structure — `production`
**VERA** `FactoryRegistry::enqueue` pushes to the FIFO tail whenever an object is held
(`factory.rs:634-642`) with no category exemption; `Building` and `Defense` are ordinary
`ProductionCategory` values. **Reference** two separate refusals: `house.cpp:2389-2393` returns
`PROD_CANT` when `fptr->Is_Building() && type == RTTI_BUILDINGTYPE`, and the queue branch
(`factory.cpp:251`) is explicitly gated `object.RTTI != RTTI_BUILDINGTYPE`, while
`factory.cpp:247-249` makes any building-type `Set` abandon whatever is in progress. Buildings
are never queued.
**TS-RISK** **yes, moderate** — RA2/YR reworked the sidebar tabs (separate Structure and
Defense columns, `MaximumQueuedObjects`), so the prohibition needs confirming in gamemd rather
than assuming the TS rule carried over. *Demoted from Tier 1 on this basis.*
**Effect** a player can stack several structures on the structure tab and walk away; in the
reference the second cameo click is refused with the scold sound. Also changes what a
right-click cancel targets. · **Frequency** every time a player clicks a second structure
cameo — several times per match.
**Ghidra** Decompile YR's production-add path and read whether building-type items can enter
the queue at all.

#### T2-27 · Particle animation-state advance uses a different denominator and no per-particle phase — `particles`
**VERA** `system_ai.rs:66-95` `advance_state` — denominator is
`(image_frame_count % 2 + 1) + StateAIAdvance` (SHP frame-count parity plus one), numerator a
per-particle counter starting at 0. `fire.rs:31-36` states this formula as the binary's,
sourced from `GetImageFrameCount()`. **Reference** the same expression appears five times in
`particle.cpp` (`:351`, `:424`, `:538`, `:600`) as
`((Class->MaxEC - RemainingEC + Fetch_ID()) % (Class->StateAIAdvance + (Fetch_ID() & 1))) == 0`
— the parity term comes from the particle's own **object ID**, not the SHP frame count, and
there is no `+1`.
**TS-RISK** no. Note the two formulas are close enough that a decompiler could plausibly
render one as the other, which is why this wants one targeted re-read rather than an argument.
**Effect** (a) every puff in a plume advances and dies in lockstep instead of at staggered
phases, so a plume pulses as one block; (b) puffs last visibly longer —
`[SmallGreySmoke]` (`StateAIAdvance=4`, `EndStateAI=20`, `ini/RULESMD.INI:26307-26317`) takes
100–120 ticks under VERA's divisor against 80–100 under the ancestor's, ~20–25% longer-lived.
· **Frequency** constant — the same 177 damage-smoke producers plus every smokestack.
**Ghidra** Decompile YR's particle state-advance and read the modulus operands from the
disassembly, not the decompiler output.

#### T2-28 · House defeat and house AI are two separate global passes — `world`
**VERA** `check_defeat(rules)` walks every house at `src/sim/world/mod.rs:5937`; only
afterwards does `ai::tick_ai` walk every AI house at `:5947`. **Reference** both live inside one
`HouseClass::AI` call — the multiplayer defeat test with `Blowup_All(); MPlayer_Defeated();`
at `house.cpp:1493`, then `Expert_AI()` at `:1592` and the AI_* passes at `:1601` — and
`logic.cpp:408` calls that once per house in `Houses` array order. House N+1's AI observes
house N's `Blowup_All` from the same frame.
**TS-RISK** no · **Effect** on the frame a house is eliminated, surviving AI houses decide
against a different world state; a simultaneous double elimination can resolve in the other
order and hand the win to the other side. · **Frequency** rare — once per house per match —
but it is the frame that decides the match.
**Ghidra** Decompile YR's per-house AI entry and confirm whether defeat and AI are interleaved
per house.

#### T2-29 · Megamission commands are hoisted into a second stream per house — `world` *(lockstep)*
**VERA** `apply_due_commands` (`src/sim/world/mod.rs:5832`) loops houses in registration order
(`:5849`), applies every non-megamission command for that house (`:5850-5872`), and only then
applies that house's staged megamission commands in a second loop (`:5874-5896`).
**Reference** `Execute_DoList` (`queue.cpp:3730`) loops houses in `Houses` array order (`:3767`)
and, within a house, walks the single `DoList` in queue order (`:3793`). MEGAMISSION events
are ordinary `DoList` entries and are not hoisted.
**TS-RISK** no · **Effect** when one player issues a move/attack order and a build-placement,
sell, or deploy in the same frame, the two commit in a different relative order — which
matters when they touch the same cell or the same wallet. · **Frequency** rare, but this is
deterministic command ordering, so a divergence here is a lockstep hazard.
**Ghidra** Decompile YR's event-list execution and confirm whether megamission events are
walked in arrival order with everything else.

#### T2-30 · The ROF jitter draw is unconditional; the ancestor skips it for sonic and particle weapons — `combat` *(determinism)*
**VERA** `combat/mod.rs:7394` `rof_to_cooldown_frames` always draws
`scenario_rng.next_range_u32_inclusive(0, 2)` and adds it to `ROF`. Its doc comment lists three
known-missing arms of `TechnoClass::GetROF @ 0x006FCFA0` (house difficulty, `VeteranROF`,
`RadialFireSegments`) — a sonic/particle early-return is not among them. **Reference**
`techno.cpp:3648-3654` — `Rearm_Delay` returns the raw `weapon->ROF` with **no** jitter, no
bias and no burst branch when the weapon is `IsSonic`, or uses fire/spark/railgun particles
with the corresponding particle system attached. `:3639-3641` adds a second early return: a
building with `Ammo > 1` rearms in 1 frame.
**TS-RISK** partial — `IsRailgun` is TS-flavoured, but `IsSonic` and `UseSparkParticles` are
authored in stock YR `rulesmd.ini`, so the branch is at least reachable
**Effect** none visible on its own — a 0–2 frame cadence difference. The real cost is
determinism: a skipped draw shifts the scenario RNG stream for everything else that tick. ·
**Frequency** per shot for the Dolphin (`[DLPH] Primary=SonicZap`, `IsSonic=Yes`) and for an
Engineer-loaded IFV (`[RepairBullet] UseSparkParticles=yes`). `[LtRail]` has no stock user.
**Ghidra** Decompile `0x006FCFA0` and enumerate which early-return arms survive into YR. One
read settles the whole entry.

#### T2-31 · Sonic wave sweeps its damage cells from the target back to the firer — `superweapon`
**VERA** `src/sim/wave.rs:387-401` — the active branch seeds `previous = lepton_cell(target)`
and walks `interpolate(target, source, t)` for `t = 0.05 … fade_in`, where `source` is the
attacker (set at `src/sim/world/mod.rs:1871-1877`). Consequence: the **target's own cell is
the seed and is never appended**, and the firer's cell is appended at `t=1`. The fade branch
(`:403-429`) is inverted the same way. **Reference** `wave.cpp:976-1021` seeds
`start = StartCoord.As_Cell()` (the firer, per `Build_Wave_Shape:1057-1069`) and walks
`Lerp(StartCoord, EndCoord, t)` upward from `t = WaveStep`, so growth runs firer → target.
Both engines otherwise agree closely (same 0.05 step, same loop bound, same `WaveEC`/lifetime
100 countdown, same laser `-6 / <32` rule, same 2172-lepton tracking break, same `!= 20` frame
gate, same firer immunity and `AmbientDamage` + warhead damage, same wall/cliff tail).
**TS-RISK** no · **Effect** during the ~20 frames the beam extends, damaged cells grow from
the wrong end, and the aimed-at unit's cell is the one cell never recorded while the wave is
active — a Dolphin's beam chews the water behind its own hull first. · **Frequency** every
engagement involving a sonic weapon, but stock YR sets `IsSonic=Yes` only on
`[SonicZap]`/`[SonicZapE]` (the Dolphin), so **naval maps only** — this is what holds it out of
Tier 1.
**Ghidra** VERA cites `WaveClass::UpdateCells @ 0x007610F0`. The resolution is specifically
"which of `WaveClass+0xB4` / `+0xC0` is the firer" — a field-identity question, exactly the
kind that inverts silently.

#### T2-32 · A crushable overlay ahead does not force a straight track, and there is no crush tilt — `movement`
**VERA** track selection is a pure function of two facings —
`plan_drive_track_from_path(...)` (`src/sim/movement/movement_step.rs:1181`) feeding
`select_drive_track` (`drive_track.rs:3516`). No overlay, crushable or rocking term is read;
`grep -rn "rocking" src/sim/` finds only the voxel/ship tilt state in `components.rs:1072`.
**Reference** `drive.cpp:1882-1886` — if the cell the curve would swing into carries a crushable
overlay, or the destination cell does, the engine overrides `nextface = facing` (drive
straight) and sets `IsRocking = true`; `:1372-1377` raises `IsCrushing` and, for
`IsTiltsWhenCrushes` types, `RockingForwardsPerFrame = -0.02f`.
**TS-RISK** low — crushable overlays exist in YR; the specific `OVERLAY_SANDBAG_WALL` identity
and `IsTiltsWhenCrushes` need YR confirmation
**Effect** a tank that would clip a fence corner curves around it instead of driving straight
through, and does not pitch forward as it flattens it. · **Frequency** per fence/sandbag line
crossed — regular in base assaults, not constant.
**Ghidra** Decompile YR's track selection and look for a crushable-overlay override of the
next facing.

#### T2-33 · The first production step fires the tick after enqueue, not one build-rate period later — `production`
**VERA** `FactoryRegistry::enqueue` seeds `step_timer = 0` and `step_rate_frames = 0`
(`factory.rs:647-648`, `:672-673`); `step_all` recomputes the rate and only decrements when
`step_timer > 0` (`:1064-1067`), so the first charge lands on the next sweep. **Reference**
`FactoryClass::Start` calls `Set_Rate(Build_Rate())` (`factory.cpp:375`), and
`StageClass::Set_Rate` sets `Timer = rate` as well as `Rate = rate` (`stage.h:73`);
`Graphic_Logic` only advances when `Timer == 0` (`stage.h:88-92`), so the first step — and the
first credit debit — lands a full build-rate period after production starts.
**TS-RISK** no · **Effect** credits leave the account immediately on click instead of after
the first step interval, and each build finishes about one step period early (~1 s for a
1000-credit item at retail rates). · **Frequency** every build, magnitude one 54th of the
total.
**Ghidra** Decompile YR's factory start and read whether the step timer is armed to the rate
or to zero.

#### T2-34 · Miner departure is not gated on the refinery's unload animation — `miner`
**VERA** `phase_departing` releases contact, clears the sprite override and queues Harvest as
soon as the empty-slot gate fires (`src/sim/miner/miner_dock_sequence.rs:1305-1400`).
**Reference** state 4 re-reads the building west of the miner and stays docked while
`Anim_Active(BANIM_PRODUCTION)` is true (`unit.cpp:3226-3250`), only then sending
`RADIO_OVER_OUT` and re-assigning MISSION_HARVEST; the anims start at dump begin
(`BANIM_PRE_PRODUCTION`, `:3200-3204`) and at the last-bale transition (`BANIM_PRODUCTION`,
`:3275-3278`).
**TS-RISK** **yes** — TS's `BANIM_*` building animation-state vocabulary is TS-era; the "wait
for the anim" *rule* is the checkable part, not the anim names. *Demoted from Tier 1 on this
basis.*
**Effect** how long the miner sits on the pad after the credits land, and whether the bay
animation and the departure are synchronised. · **Frequency** constant — every dock cycle.
**Ghidra** Decompile YR's harvester unload exit state and read what, if anything, it waits on
before releasing the bay.

#### T2-35 · The engineer-cursor bridge repairability test walks overlays where the ancestor walks zone connections — `bridge`
**VERA** `bridge_hut_has_collapsed_span` (`src/sim/world/bridge_orchestrator.rs:324`) always
takes the overlay branch: scan the 5×5, follow the band perpendicular to the overlay class,
return true on a collapsed anchor (`0xE7`/`0xE8` high, `0x64`/`0x65` low). Its own doc comment
(`:307-323`) records the record-walking alternative as unimplemented and its answer as
UNCHECKED. **Reference** `MapClass::Can_Repair_Bridge` (`map.cpp:12104`) sets `found = true` only
on a bridge/train **iso-tile** match; when found it derives the tile's top-left from
`SubTile % width` / `SubTile / width`, offsets into a 16-entry `_x`/`_y`/`_facings` table, and
walks `ZoneConnections` via `Zone_Connection_Index(search, 3, 0)`, returning true the moment
`!con->IsPassable` (`:12146-12175`). Only the `!found` low-bridge case walks overlays
(`:12180-12222`).
**TS-RISK** no. This resolves VERA's own open question in the direction "the high case takes
the record branch".
**Effect** the engineer repair cursor and the repairability verdict over a hut whose HIGH span
is broken. · **Frequency** every engineer mouse-over of a bridge repair hut.
**Ghidra** Decompile `0x00587410` and read which of the two branches the high-bridge case
takes.

#### T2-36 · Falling/parachute descent runs after the object's mission and locomotion; in the ancestor it is first — `world`
**VERA** `parachute_descent::tick_parachute_descent_in_order` is called at
`src/sim/world/mod.rs:6501` — after that object's mission dispatch (`:6353`), after its ground
locomotor (`:6379`), and after the teleport/rocket/tunnel/drop-pod leaves. **Reference** the
falling integration is `ObjectClass::AI` (`object.cpp:268`) — `Height += Riser.Z`, the
`HeightAGL <= 0` landing that calls `Per_Cell_Process(PCP_END)` and `Shorten_Attached_Anims`,
and the `IsToExplode → Take_Damage` on touchdown. It is the **first** thing reached in the
base chain, from `MissionClass::AI`'s `BASECLASS::AI()` (`mission.cpp:223`), before the
mission switch at `:241`.
**TS-RISK** no for paradrop and low-bridge drop-in; the same `ObjectClass::AI` body also
serves jumpjet/dropship descent, which is TS-only, so isolate the paradrop path before porting
anything from it.
**Effect** a paradropped or bridge-dropped object touches down, marks its cell and takes its
landing damage before its mission runs in native, so it can act on its landing frame; in VERA
the landing resolves after that frame's mission dispatch. · **Frequency** per paradrop and per
bridge collapse — rare in a stock 1v1, every engagement in a paradrop-heavy game.
**Ghidra** Verify in the same spine pass as T1-17/T1-18: where does YR's object base AI put
the fall integration relative to the mission switch?

#### T2-37 · Superweapon charge and delivery sit at the head/middle of VERA's frame — `world`
**VERA** `tick_active_superweapon_effects` runs at the very top of the frame, before any
object AI (`src/sim/world/mod.rs:6294`), and `tick_superweapon_instances` (charge/ready) runs
as Phase 4.5 (`:6691`), before combat. **Reference** `Super_Weapon_Handler()` is called from
inside `HouseClass::AI` (`house.cpp:1485`), i.e. in the frame tail after the whole object walk
(`logic.cpp:365` then `:408`); the delivery itself fires from the owning building's own turn
(`building.cpp:5894`).
**TS-RISK** no for nuke / Iron Curtain / Chronosphere / Lightning Storm. `IonStormClass::AI`
(`logic.cpp:357`) and `EMPulseClass::Update_All` (`:359`) in the same neighbourhood are
TS-only — do not port their slots.
**Effect** superweapon damage applied before any object has moved hits targets at their
previous-frame positions; native applies it mid-walk, so units at the blast rim live or die
differently. The ready transition also lands on the other side of that frame's production
step. · **Frequency** the charge tick is constant; the delivery is a handful of times per long
skirmish, and each one is decisive.
**Ghidra** Same spine pass: locate YR's superweapon handler call site relative to the object
walk.

#### T2-38 · `AmphibiousDestroyer` × partially-blocked passability cell — `pathfinding`
**VERA** `pathfinding/passability.rs:54`, row 3 (`AmphibiousDestroyer`), column 5 (the
infantry-admitting "partially blocked" column) = `1` (passable). Sourced by VERA from the
native 13×8 table at `0x0082A594`. **Reference** `map.cpp:173`,
`MZONE_AMPHIBIOUS_DESTROYER × PASSABLE_PARTIALLY_BLOCKED = TRAVERSAL_IMPASSABLE`. Every other
cell of the ten shared rows agrees column for column once the YR-inserted Beach column is
accounted for — this is the single disagreement in the table.
**TS-RISK** **yes**, and the risk points at the reference tree, not VERA: the stock `rulesmd.ini` comments
on `[GHOST]` and `[TANY]` explicitly reference the "seal stuck on tree bug", which reads like
a deliberate TS→YR change to this exact row.
**Effect** zone building and reachability for the four stock users — `[GHOST]` (Navy SEAL),
`[TANY]` (Tanya), `[YURIPR]` (Yuri Prime), `[ROBO]` (Robot Tank): they either can or cannot
treat infantry-only broken terrain as connected ground. · **Frequency** per order for those
units — SEAL/Tanya in most Allied games, Robot Tank in most Yuri games.
**Ghidra** One byte: read `0x0082A594 + 3*8 + 5`. Listed because it is cheap, not because VERA
looks doubtful.

---

# TIER 3 — edge cases, internal-only, and latent-until-a-feature-lands

One line each. All LEAD unless noted. All still require the TS-reachability gate.

| ID | System | Difference | TS-RISK | Ghidra question |
|---|---|---|---|---|
| **T3-01** | `bridge` | Low-bridge damage advances **one** overlay stage per gated hit (`bridge_orchestrator.rs:1884` sets `max_attempts = 1`; `walker.rs:509` advances one stage); `combat.cpp:525-531` calls `Damage_Low_Bridge(cell)` **twice** after a single BridgeStrength gate, discarding the second return. Effect: a low bridge takes ~2× the shells. Frequency: every attack on a low bridge. | low but real — reads like a TS-era duplicated call YR may have cleaned up | Decompile the YR caller and count the handler invocations per gated hit. |
| **T3-02** | `bridge` | Bridge repair does not detach or scatter other infantry queued into the hut. VERA (`world_orders.rs:317-480`) consumes only the arriving engineer; `infantry.cpp:800-805` runs `Infantry[i]->Detach(tech,false)` over **every** infantry then `Scatter_Incoming_Infantry()`. Effect: a second engineer already ordered onto the hut walks in and is consumed for nothing — total loss when it fires. Frequency: rare (two engineers at one hut). | no | Decompile YR's engineer bridge-repair tail and look for a detach/scatter sweep. |
| **T3-03** | `docking` | HELLO does not tear down the sender's existing link first. `radio/mod.rs:153-184` `insert_evicting` overwrites slot 0 silently, leaving a one-sided link (acknowledged at `:177-179`); `radio.cpp:247-254` unconditionally transmits `RADIO_OVER_OUT` to itself before dispatching HELLO. **Dormant today** — VERA's refinery FSM always BREAKs before re-HELLOing — becomes constant the moment any multi-dock or transport caller re-targets without a BREAK. | no | Decompile YR's HELLO transmit and check for a self-directed teardown. |
| **T3-04** | `docking` | The tether is reciprocal in the ancestor, one-sided in VERA. `radio/receive.rs:122-137` writes only the sender's `dock_entered_with`; `techno.cpp:984-1001` sets the *receiver's* `IsTethered` then transmits TETHER back, guarded by `if (!IsTethered)`, with UNTETHER the exact mirror and explicit teardown ordering at `:1015-1018`. **Latent** — nothing in VERA reads the flag on the building side; becomes visible the moment a tether-gated behaviour lands (T2-08). | no | Decompile the YR tether/untether cases and read the reciprocal transmit. |
| **T3-05** | `docking` | The radio opcode table's tail appears shifted by two, and `0x10` is omitted as "not a wire message". `radio/mod.rs:216-243`, 12 entries marked `name inferred`. `radio.hh:22-60` lines up exactly in the low half (incl. `0x0E` carrying the hardcoded refinery pad cell, `building.cpp:533`), then reads offset by two: `RepairTick=0x1C ↔ REPAIR(0x1A)`, `IsRepairing=0x22 ↔ NEED_REPAIR(0x20)`, `IsOccupied=0x23 ↔ ON_DEPOT(0x21)`, `InsufficientFunds=0x20 ↔ CANT(0x1E)`, `RepairComplete=0x21 ↔ ALL_DONE(0x1F)`. That places the two inserted YR ordinals at `0x1A`/`0x1B`, exactly where VERA has the unexplained `SecondaryLockSet`/`SecondaryLockClear`. Two entries do not fit: `0x0D AnimStop` (ancestor `REDRAW`) and `0x1F LinkPassenger` (ancestor `RELOAD`). `0x10` is a real ancestor receiver case — `ARE_REFINERY`, `building.cpp:564-568`. No player effect; **gates the correctness of T1-15, T2-23 and T3-04.** | yes for the shift hypothesis — the two inserted ordinals are by definition RA2/YR additions and cannot be identified from the reference tree at all | Enumerate the YR `Receive_Message` switch cases by ordinal in `BuildingClass` and `TechnoClass`. Cheapest naming fix in the ledger; do it before more handlers land. |
| **T3-06** | `aircraft` | **Rocking kernel semantics — four sub-differences, one Ghidra pass.** All latent behind T1-04. (a) *Frame:* `impulse.rs:85-105` never reads the unit's `facing` and **adds** onto existing velocities with a ±0.05 clamp; `techno.cpp:7905-7932` projects the jolt onto the unit's own facing frame (`As_Radian()`, sin/cos, sign-recovery flip at `:7928-7930`) and **assigns** rather than accumulates, gated to voxel objects only (`:7897`). (b) *Attenuation:* `impulse.rs:14` fixes `FORCE_COEFFICIENT = 0.04` (deferral recorded at `:12-13`); `techno.cpp:7911` is `(0.04f - dist1*0.000025f) * force / Weight`, reaching zero at ~6.25 cells. (c) *Damping:* `rocking_system.rs:111-126` keys on velocity sign with one rate and returns to level only via a deadband; `techno.cpp:7947-7998` latches the **pre-integration angle sign** and decays 0.002 outbound / 0.005 inbound — an asymmetric restoring spring — with a zero-crossing clear. (d) *Clamp:* `rocking_system.rs:95-119` gates the ±π/4 clamp on `!is_moving` and adds an out-of-±π/2 runaway branch; `techno.cpp:7969-8020` clamps unconditionally whenever the axis velocity is nonzero and zeroes it, tightening the forwards cap to π/10 for a crushing `FootClass`. | no for the clamp. **The ±π self-destruct and the runaway branch are VERA claims with no the reference tree analogue at all — treat their *presence* as the thing needing Ghidra proof, not their absence here.** | In the same pass as T1-04: decompile YR's `Rock` and `Rocking_AI` and recover the frame projection, the attenuation term, the two decay rates, and the clamp conditions. |
| **T3-07** | `movement` | Jumpjet wobble is lateral and render-only. `jumpjet_movement.rs:84-96` `compute_wobble` returns an X/Y **screen-pixel** offset from an invented phase formula, labelled "render-only"; altitude (`:122`) is a plain ramp with no wobble and no climb penalty. `jumpjet.cpp:523-528` accumulates `CurrentWobble += 2π/(15/JumpjetWobblesPerSecond)` and sets `desired_height = sin(CurrentWobble)*JumpjetWobbleDeviation + FlightLevel` — **vertical** — which then gates the climb/descend decision (`:539-565`) and applies `CurrentSpeed *= 0.9` below half and again below a quarter of desired height (`:566-573`). Effect: a Rocketeer drifts sideways at fixed altitude instead of bobbing and visibly losing speed while climbing out. Frequency: every jumpjet move. | **yes, partial** — `JumpjetWobbles`/`JumpjetDeviation` exist in YR rules, but TS's five-state jumpjet machine and its ion-storm/firestorm arms are TS-side. The wobble's *axis* and its *speed feedback* both need YR confirmation before anything is built. *Demoted from Tier 1 on this basis.* | Decompile YR's jumpjet locomotor altitude step: is the wobble applied to Z, and does it feed back into speed? |
| **T3-08** | `pathfinding` | The hierarchical retry bans the whole corridor instead of the links at the choke point. `zone_search.rs:589-627` `exclude_corridor_edges` removes **every** edge of the corridor tried and re-runs the Dijkstra (`MAX_CORRIDOR_RETRIES = 5`); VERA already tracks the choke cell (`HierarchyProgressTracker::progress_cell`, `core.rs:369`) but does not use it. `astar.cpp:1823` → `Ban_Blocked_Subzone_Edges` (`:1851`) starts from `HierLastNodeCell` (`:463-466`) and, at every subzone level, tests that subzone's links with `Test_Cell_Walk` and bans **only** the unwalkable ones; the next pass consults the ban list at `:1673`. Effect: a route retail recovers on the second attempt comes back "unreachable". Frequency: the mechanism fires on any corridor A* failure, but VERA's own header notes its Dijkstra fallback is only reached on maps with authored tubes, so observable VERA-side frequency is narrower today. | no | Verify `0x0042CCD0` against the ban-list shape before ranking this higher. |
| **T3-09** | `pathfinding` | Tube edge is priced by tunnel path length, not entrance-to-exit distance. `core.rs:1550-1552` charges `STEP_COST * tube.path_len()` plus `TUBE_DIR_TIEBREAK`; `astar.cpp:429-437` computes `max(|dx|,|dy|)` entrance→exit and is the one branch that skips both the cost multiplier and the per-facing tie-break. Effect: a winding tube looks proportionally more expensive, so units route around it. Frequency: rare — tube maps only, and only where a tube competes with a surface route. | no — `TubeClass` low-bridge movement is active in YR; this is not subterranean locomotion | Decompile YR's A* tunnel branch and read the cost expression. |
| **T3-10** | `miner` | Refinery selection is nearest-only. `miner_system.rs:1751-1770` runs one global pass, strictly nearest wins; `unit.cpp:3531-3537` iterates the unit type's `Dock[]` list in order and takes the first entry yielding any bay (dock-type order outranks distance across types), and inside the scan `building->IsLeader` overrides the distance comparison entirely (`techno.cpp:7667`). Effect: which refinery miners converge on with two refineries, or two refinery types (e.g. a captured enemy refinery). Frequency: per return, once a player owns 2+ refineries. | **yes** — `IsLeader`/primary semantics for refineries may not survive into YR, and the `Dock[]` multi-entry case may be TS-shaped (weeder/refinery split) | Decompile YR's harvest bay search and read the iteration order and any primary-building override. |
| **T3-11** | `miner` | Idle exit when no ore is in range. `miner_system.rs:1339-1344` `WaitNoOre` re-scans every 105 frames indefinitely and never leaves the Harvest handler (header records the native tail as unimplemented); `unit.cpp:3462-3466` sets `IsUseless` and `House->IsTiberiumShort`, then on the next dispatch assigns MISSION_REPAIR if a repair bay exists, else MISSION_HUNT, nudges the miner off a refinery it is standing on, and drops to MISSION_GUARD (`:3585-3601`). The Guard override that would bring it back fires only for non-human houses (`:4366-4390`), which does not match the player-arm behaviour VERA's own note attributes to gamemd. Frequency: rare — only when no ore remains within 48 cells. | **yes** — the MISSION_HUNT/repair-bay tail and `IsTiberiumShort` are TS-flavoured, and the human/AI split differs between the two readings | Decompile YR's harvest GOINGTOIDLE arm and read the human-house tail specifically. |
| **T3-12** | `miner` | No weighted patch selection for multi-harvester spread. `miner_system.rs:2022` runs one scan for every miner, so co-located miners deterministically pick the same cell; `foot.cpp:4121-4125` defers to `Search_For_Tiberium_Weighted` for non-human houses in skirmish/MP, which builds a discrete distribution over ring runs scaled by the house's harvester count and samples it (`:4191-4280`), explicitly so harvesters do not pile onto one patch. **Zero frequency today — VERA has no AI houses and the gate is AI-only.** Revisit when the AI opponent lands. | no for the mechanism; the *gate* (`Session.Type != GAME_NORMAL`) needs checking | Deferred with the AI work. |
| **T3-13** | `mission` | The weapons-factory readiness gate carries an extra Enter exemption. `readiness.rs:187-197` blocks when radio slot zero is a `weapons_factory` **and** `queued != MISSION_MOVE` **and** `queued != MISSION_ENTER`; `unit.cpp:6004` has the MOVE exemption only (the ENTER exemption exists one gate earlier, on the moving test at `:5990`, which VERA also has at `readiness.rs:164`). Effect: a unit still linked to the war factory with a queued Enter commences here and holds there. Frequency: rare — needs a queued Enter while still linked to the producing factory. | no | Decompile YR's `UnitClass::Ready_To_Commence` and read the factory-contact arm's exemption list. |
| **T3-14** | `mission` | Team-script cursor advance costs an extra frame. `team_script_vm.rs:617-625` clears `advance_pending`, bumps the cursor, and `continue`s, so the newly-selected action executes only on the following update — each script step costs two passes. `team.cpp:586-604` does the advance and the execution in the **same** pass and falls straight through to the mission switch at `:634`. **Zero frequency today** (no AI opponent); constant once team scripts execute, with cumulative drift. | **yes** — TS's `TeamMissionClass`/`TMISSION_*` table is not YR's 0..=64 ScriptType action set, and the surrounding `TeamClass::AI` body is visibly different from what VERA cites at `0x006E9140`. Only the advance-then-execute *ordering* is worth carrying forward. | Confirm against YR's own `ScriptClass::HasNextMission` path when AI teams land. |
| **T3-15** | `production` | No `MaximumQueuedObjects` cap and no refusal feedback. `factory.rs:637` pushes unconditionally; `factory.cpp:252-259` adds only when `QueuedObjects.Count() < Rule->MaximumQueuedObjects && !House->Is_Build_Limited(&object)`, else plays `Rule->ScoldSound` for a player house and fails. Stock `RULESMD.INI:423` carries `MaximumQueuedObjects=29`. Frequency: rare today (needs 30 clicks on one tab), **but becomes ordinary the moment T1-05 is resolved and mass-queueing while poor is possible again.** | no — the key is present in the stock YR INI | Decompile YR's queue-add and read the cap test and the refusal sound. |
| **T3-16** | `particles` | The smoke spawn path does not age: no translucency cutoff, no per-spawn speed decay. `smoke.rs:81-112` spawns at cadence and advances `spawn_timer` but applies neither (both listed deferred at `:12-13`, speed coefficient recorded as `delta × 0.025`); `partsys.cpp:507-513` applies `Translucency += 25` past `SpawnTranslucencyCutoff` and `Speed -= (SpawnFrames - Class->SpawnFrames) * 0.35f` floored at 2.0 — coefficient **0.35**, not 0.025. Separately the two-child `NextParticle` successors each get `Translucency + (RandomNumber % 6 != 0 ? 25 : 0)` (`:474`, `:486`), two RNG draws VERA does not consume. Effect: a long-burning plume never thins or slows. Frequency: ~1,000 ticks into a system — routine for smokestacks and refinery vents. | no for the spawn-path aging. **The two-child successor arm is effectively dormant in stock YR** — no smoke particle type sets `NextParticle=`, only the gas clouds do — so treat that half as low priority. | Decompile YR's particle-system spawn and recover the translucency step and the speed coefficient. |
| **T3-17** | `vision` | Radar-spy shared-vision routing absent. VERA's only spy vision path is `FogState::reset_explored_for_owner` (`vision/mod.rs:969`), documented as resetting an *enemy's* map knowledge; `map.cpp:1081-1084` redirects a house's sight to the player whenever `house->RadarSpied` carries the player's bit, and `house.cpp:7990-8003` additionally runs a one-shot `Sight_From` at every object that house owns. Effect: in the ancestor the spy's owner sees whatever the spied house's units see, permanently. Frequency: once per successful radar infiltration — rare, decisive. | **yes.** YR appears to have replaced radar infiltration's effect with resetting the *victim's* shroud — which is exactly what VERA models. This lead may be describing TS-only behaviour. | Read gamemd's radar-infiltration handler before treating the absent routing as a gap at all. |

---

# MATCH-UNCHECKED — where the two engines agree

**These rows verify nothing.** They record that a mapper looked and found no difference, which
makes these low-yield places to point the next decompile pass — and nothing more. Do not cite
any row here as evidence that VERA is correct, in a comment, a commit message, or a review.

| ID | Group | Agreement found (recorded, upgrades nothing) |
|---|---|---|
| MU-01 | `bridge` | Debris RNG draw sequence (95% gate → 2 jitter draws → 50% metallic gate → metallic slot → 1..5 delay → explosion slot), identical including MSVC right-to-left argument order; `IsWallDestroyer`/`Wall=yes` requirement; the `roll < damage` sense of the BridgeStrength gate; the `DestroyableBridges` outer gate; the low/high overlay transition tables; the 5×5 hut scan being Y-major with no break so the LAST match wins; the `>= 2` height delta separating deck from ground in A*. |
| MU-02 | `movement` | `TRACK_STEP_COST = 7`; path buffer 24; `PATH_RETRY` 10 = `PATH_STUCK_INIT` 10; the `just_started` zero-budget re-entry after a track install; the `Accelerates=` ramp arm order and its asymmetric accel/decel; `Set_ROT` clamp to 127; the `FacingClass` interpolation formula including remainder absorption; the 256→8 facing quantiser (algebraically identical at every boundary); the exact-zero→0.5 terrain substitution and the ×0.75 damaged-mover factor; the `CellClass::Incoming` scatter dispatch gate. |
| MU-03 | `pathfinding` | Direction tie-break table and index order; the 65527 node budget; the 24-step segment; close-on-generation with no relaxation; the 1/1000/1/1/60/20/8/10000 cost-class base table; the 10-hop code-2 chain walk with 1/4/1000 outcomes; the ×4 predicted-path marker; 2×2/4×4/8×8 aligned hierarchy blocks; the height-step `< 2` zone rule; the deck-layer test; the entire collision-avoidance marker producer in `path_markers.rs`; nine of the ten shared rows of the passability matrix (the tenth is T2-38). |
| MU-04 | `combat` | Cluster scatter draw (256..512 radius plus an angle byte); half/red result classification including the overkill clamp ordering; the `AnimList` bucket size `damage / 25`; the sonic one-wave gate. |
| MU-05 | `docking` | The low half of the radio opcode table lines up exactly, including semantics: `0x02 HELLO`, `0x03 OVER_OUT`, `0x0A NEGATIVE`, `0x12 MOVE_HERE`, `0x13 NEED_TO_MOVE`, and `0x0E` carrying the hardcoded refinery pad cell. (The tail is T3-05.) |
| MU-06 | `miner` | Ring-expanding scan with per-ring early exit and best-value-in-ring; cell value `Value × (data+1)`; the reduce partial predicate `data+1 > levels`, the full-clear returning the pre-removal density, the growth queue at max density, and the spread-state clear plus 8-neighbour reseed; 9 load steps × `HarvesterLoadRate` before each lift; the east-facing pivot before dumping with a 5-frame wait. |
| MU-07 | `mission` | **MissionControl resets per entry with no carry-forward** (one object per slot, defaults constructed, `Read_INI` returns untouched when the section is absent) and **AARate absent or 0 copies Rate** — the two questions the sweep was asked to settle, both "agrees". Also: the Queue write predicate; the Override suspended-slot choice; that Override does not clear the queue; the Restore reset set; the `Random_Pick(0,2)` handler cadence jitter and `RandomRanged(1,5)` on the Area Guard tail; the building Guard delay selector (armed → AA_Delay, unarmed repair-bay → Normal_Delay, unarmed otherwise → Normal_Delay × 3). |
| MU-08 | `production` | The 54-step count; the per-step instalment `balance / (54 - progress)` computed after the increment; the strict `>` affordability comparison so an exactly-affordable step proceeds; the shortfall rewinding one step with no spend; the zero completion residual; cancel-of-a-queued-copy removing the first front-to-back match with no refund; the queue advancing on cancel of the active build; the rate refresh without disturbing the running countdown; one step per tick maximum; the prerequisite-loss sweep dropping queued entries without refund, abandoning the active build with a refund, then promoting. |
| MU-09 | `aircraft` | No independent agreements recorded beyond the homolog mapping; every examined mechanism produced a lead (T1-03, T1-22, T1-23, T2-09) or is latent behind T1-04 (T3-06). |
| MU-10 | `vision` | The 8-bit neighbour bit layout matches `Cell_Shadow`'s index construction bit for bit; the `+2` obstruction bias matches `Sight_From`'s `hoffset` algebra; the `min(sight, 10)` clamp, the `sight == 0` early-out, the `viewer_level + 3` LOS threshold, and the multiplicative elevation scale all match; the Z-shift divisor matches once normalised for RA2's 60×30 tiles; the extra-pixel threshold and `+0.5` truncation match; permanent `FLAG_REVEALED` matches (`IsMapped` is never cleared outside `Shroud_The_Map`); buildings reveal from the NW corner in both. **This is why the whole vision group ranks Tier 2 — the mechanism a player looks at every frame has no lead against it.** |
| MU-11 | `superweapon` | Sonic waves otherwise agree closely with `wave.cpp`: same 0.05 step, same `+WaveStep/2` loop bound, same `WaveEC`/lifetime 100 countdown, same laser `-6 / <32` rule, same `CELL_LEPTON_DIAG*6 = 2172` tracking break, same `!= 20` frame gate, same firer immunity and `AmbientDamage` + warhead damage, same wall/cliff tail. Only the sweep direction diverges (T2-31). |
| MU-12 | `world` | Pending-delete drain shape (preserve non-ready, collapse duplicate ready ids, finalize once, run after the frame-counter commit); logic-vector registration (unsorted tail-append, first-match compacting remove); no index repair on mid-pass removal (the reference tree's `index--` repair is commented out at `logic.cpp:376`); synchronous limbo/uninit membership release with pointer-expiry broadcast twice on both sides; command dispatch after the object walk with only `SetGameSpeed` at head-of-frame; tube movement owning the whole object turn; ore growth before the object walk; team AI before the object walk; factories before houses. |

---

# TS-DIVERGENCE — closed, do not implement

the reference behavior here is TS-only or superseded. VERA's difference is expected. These rows
exist so a future pass does not "discover" them again as gaps.

| ID | Subject | Why closed |
|---|---|---|
| TD-01 | TS damage falloff (`SpreadFactor`, inverse-divide, 0..16 clamp, `Rule->MinDamage` floor when `distance < 4`) — `combat.cpp:134-153` | YR replaced the key and the formula with `CellSpread`/`PercentAtMax` linear lerp, which VERA implements (`damage/kernel.rs:86-99`). Adding the the reference tree floor would be the drift. `MinDamage=` is parsed and never read by gamemd (exhaustive operand scan recorded at `damage/kernel.rs:40-48`); stock authors it `;gs obsolete`. |
| TD-02 | TS-era `VeteranArmor` / `VeteranROF` reciprocal form `1.0/(x+1.0)` — `techno.cpp:4880`, `:3668` | Stock YR authors `VeteranArmor=1.5 ; damage is divided by this` and `VeteranROF=0.6 ; ROF delay multiplier`, matching VERA's plain divide. (VERA's *absence* of `VeteranROF` is a separate recorded residual at `combat/mod.rs:7378`.) |
| TD-03 | TS-only combat content: Tiberium chain reaction (`combat.cpp:167`), veinhole monsters (`:379-386`, `techno.cpp:5146`), `IonStormWarhead` team immunity (`combat.cpp:409`), `IsTiberiumHeal` death spew (`techno.cpp:4956`), cyborg-survives-at-25%-and-goes-prone (`object.cpp:1684-1699`), `Verses`-collapses-to-1 (`combat.cpp:126-129`) | All TS-only or superseded; correctly absent from VERA. `Verses=0%` meaning true immunity is the YR behaviour. |
| TD-04 | Low-power production **staircase** (1.0/0.75/0.5 buckets, `techno.cpp:797-803`) and the TS **MultipleFactory** single-divide form (`techno.cpp:806-809`) | No RA2/YR counterpart. Stock `RULESMD.INI:369-371` carries `MinLowPowerProductionSpeed`, `MaxLowPowerProductionSpeed`, `LowPowerPenaltyModifier`, which is what VERA models. `RULESMD.INI:368` explicitly documents the RA2 MultipleFactory change to a cumulative multiplier, i.e. the the reference tree form is the superseded one. |
| TD-05 | `IsAvoidBridges` 10×/2× bridge cost multiplier (`astar.cpp:208-247`); hierarchical threat-avoidance scoring (`:1663-1670`); `IsTrain`/`IsPassive` gates (`:299-358`, `:360`, `:467`) | `IsAvoidBridges` is never set true anywhere in the reference tree, and VERA records the YR counterpart as constructor-zero with no writer. Threat-avoidance and the train/passive keys have no driver in stock `rulesmd.ini`. All dormant. |
| TD-06 | Low-power structure damage during brownout (`house.cpp:1396`, `DamageDelay=1` present at retail `rulesmd.ini:59`) | Already refuted against gamemd: the `HouseClass+0x578C/+0x5794` timers are written in the constructor and never read (`src/sim/power_system.rs:144`), pinned by a test at `power_system.rs:544`. **Do not reopen.** |

---

# NOT-HOMOLOGOUS — the reference tree cannot inform these

YR-only content, or VERA-internal architecture with no Westwood counterpart. Any question here
goes straight to Ghidra; there is no lens to look through.

| ID | Subject | Why |
|---|---|---|
| NH-01 | `src/sim/movement/teleport_movement.rs` | Chrono/warp locomotion is RA2/YR-only. |
| NH-02 | `src/sim/movement/tunnel_movement.rs`, `drop_pod_movement.rs` | No TS counterpart in the compared form (TS drop pods are `super.cpp:882`, a different mechanism). |
| NH-03 | `src/sim/movement/locomotion/*`, `src/sim/substrate/locomotion/*` | VERA-internal locomotor slot/piggyback architecture. `tube_movement.rs` DOES have a homolog (`tube.cpp` + the tube arm in `drive.cpp::While_Moving`) and is not listed here. |
| NH-04 | `src/sim/combat/base_defense_response/`, `combat/inviso_scatter.rs`, `combat/threat_range.rs` YR mission table, `combat/smudge_dispatch.rs` | YR-only content or VERA-internal architecture. |
| NH-05 | Mind-control / Ivan-bomb / temporal / parasite arms of `sim/projectile.rs:557` | YR-only. |
| NH-06 | `src/sim/docking/pad_geometry.rs` | `DockingOffset`/`NumberOfDocks` multi-pad airfields are an RA2/YR addition; TS helipads hold exactly one occupant encoded as the single `Radio` pointer. |
| NH-07 | `src/sim/docking/bunker_install.rs`, `bunker_link.rs` | `Bunker=yes` tank bunkers (NATBNK) are YR content. |
| NH-08 | Refinery-side unload machine | `BuildingClass::Do_MISSION_HARVEST` (`building.cpp:5304`) is `#if 0`'d out in TS; unloading lives entirely on the unit side, as in VERA. Vein harvesting (`vein.cpp`, every `IsToVeinHarvest` branch) is TS-only. |
| NH-09 | The `Deliberate(28)` / Wait mission, the two Paradrop and two Spyplane missions | TS's mission table ends at Patrol. VERA's `Deliberate → Guard` guard and the Aircraft `mission 0x1E` exemption to the locked-heading latch have nothing to compare against. |
| NH-10 | `src/sim/aircraft/runtime_contract.rs` | VERA-internal frame-latch scaffolding. |
| NH-11 | `src/sim/aircraft/drop_payload.rs`, `paradrop_mission.rs` | YR superweapon paradrop. TS `Paradrop_Cargo` (`aircraft.cpp:1041`) is scenario-reinforcement cargo with no cadence — comparing them would import TS legacy. `dropship.cpp` deliberately not consulted. |
| NH-12 | `src/sim/superweapon/genetic_converter.rs`, `psychic_reveal.rs`, `force_shield.rs`, `iron_curtain.rs`, `paradrop.rs`; `wave.rs` type 3 (MagBeam) | Yuri psychic/genetic content is YR-only; TS's nearest analogues to shield/curtain are the Firestorm wall and drop pods, both TS content. TS's `wave.cpp` knows only SONIC, LASER, BIG_LASER. `ionblast.cpp` (Ion Cannon) is TS-only. |
| NH-13 | `src/sim/particles/spark_world.rs`, particle store/serde plumbing, the `system_ai.rs` take/reinsert ownership dance | VERA-internal architecture. |
| NH-14 | `src/sim/map/` (`mod.rs`, `bridge_topology.rs`, `bridge_occupancy_shadow.rs`) | Not homologous *to the vision group's homolog set* — it is a bridge-topology / two-layer occupancy read service, not a cell or shroud master. the reference tree's bridge code lives in `map.cpp` but belongs to the movement/terrain groups, which are covered by the `bridge` entries. |

---

# Frozen scope

**This ledger is frozen at 83 leads as of 2026-08-27.** It is the complete output of the
twelve-lane sweep, and it is not a backlog, not a work queue, and not a parity tracker
(`AGENTS.md` forbids hand-maintained parity ledgers; this document is explicitly a *lead
inventory* with no completion state and no status column that anyone is meant to tick).

Rules for later work:

1. **Do not add entries.** Anything discovered after the freeze is a **residual** — recorded
   at its own site with trigger, player effect, and frequency, per `AGENTS.md` — unless
   correctness of an entry already in this ledger requires it, in which case it is folded into
   that entry's Ghidra question rather than given a new ID.
2. **Do not re-rank on prose.** Ranking moves only when a Ghidra pass changes an entry's
   disposition or its TS-RISK. Two entries carry pre-agreed promotions: **T2-13/T2-14 promote
   to Tier 1 if gamemd's `RevealOnFire` default is `yes`**, and **T3-15 promotes if T1-05
   lands**. Nothing else promotes without a decompile citation.
3. **When an entry is verified, edit its disposition in place** to `VERIFIED-DRIFT` with the
   inline Ghidra address, or to `TS-DIVERGENCE` if the TS-reachability gate closed it, or
   delete nothing — a closed entry stays visible so the next session knows it was answered.
4. **When a fix lands, the entry stops being the record.** The record becomes the test and the
   provenance comment, both citing gamemd. Link the entry ID from the commit if it helps;
   never cite this document or the reference tree as the reason for the change.
5. **Scan findings decay in days.** Before implementing from any entry here, check
   `git log --grep` and the live code — several of these may already have been fixed by
   parallel work.
6. Entries whose IDs disappear from a future version of this file were merged, not dropped
   silently. Two leads were dropped at freeze as too weak to carry: `production` LEAD 8
   (abandon refunds against a start-time cost snapshot rather than the live
   `Cost_Of(house)` — no visible effect while a house's cost bias is constant, and no known
   stock-YR trigger that changes it mid-build) and `docking` LEAD 6 (dock reservation as a
   side-table rather than the radio link itself — no player effect; VERA's `cleanup_dead` and
   per-tick depot re-validation already cover the destroyed-mid-link case).
