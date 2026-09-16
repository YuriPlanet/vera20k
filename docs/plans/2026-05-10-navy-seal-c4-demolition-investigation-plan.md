# Navy SEAL / Tanya C4 Demolition Attack — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass. Execute by running
> `/re-investigate navy seal tanya c4 demolition` with this plan loaded as
> context, OR by dispatching the function inventory to subagents in batches of
> 5–8.

**Topic:** The walk-up-and-detonate building-demolition mechanic used by Navy
SEAL, Tanya, and (to a lesser extent) Yuri. *Distinct from* Crazy Ivan's timed
BombClass — already documented separately in [BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md).

**Scope Size:** Medium — ~17 functions, 14 INI keys, 3 known offset conflicts to
resolve, 5 confirmed flag offsets to disambiguate.

**Est. Effort:** ~5–7 hours of `/re-investigate` work
- 7 FULL-depth functions × ~25 min = ~3 h
- 6 MEDIUM-depth functions × ~8 min = ~50 min
- 4 LIGHT-depth functions × ~4 min = ~15 min
- INI cross-reference + offset reconciliation + writeup = ~1.5 h

**Prior Research:** Partial — see Section 2. No dedicated SEAL/Tanya C4 doc
exists. Several reports cover *adjacent* systems (Mission_Capture handler,
ANIMCLASS RING1, building destruction, warhead detonate) and mention C4 in
passing, often with conflicting offsets.

**Expected Output:**
[docs/research/NAVY_SEAL_TANYA_C4_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVY_SEAL_TANYA_C4_GHIDRA_REPORT.md)

**Next Pipeline Step:** `/brainstorm navy seal c4 demolition implementation`,
then `/write-plan` once the design is settled. The implementation itself is
small (one new attack mode in InfantryClass mission/fire logic, one new flag on
InfantryType, one new flag on BuildingType, hooked into existing cursor
plumbing). The investigation is the load-bearing step.

---

## 1. Goal

When this investigation finishes, the resulting research document must answer:

1. **End-to-end pipeline.** From the moment a player right-clicks an enemy
   building with a SEAL/Tanya selected, to the moment the building reaches 0
   HP, what is the exact sequence of mission states, animation DoTypes, and
   damage calls?
2. **Flag offsets.** What are the verified offsets on `InfantryTypeClass` and
   `BuildingTypeClass` for `C4=`, `CanC4=`, and the related Engineer/Spy/Agent
   flags that share parser logic (multiple prior reports disagree —
   reconcile)?
3. **Damage application.** Does C4 damage flow through `Apply_area_damage`
   with `Rules->C4Warhead` (= `Super`), or does it call `Take_Damage` directly
   on the building, or both, and what are the conditions?
4. **Self-destruct.** When/why does the SEAL/Tanya destroy themselves on a
   C4 plant? Always? Only if `Suicide=yes` on weapon? Verify against gamemd's
   actual behavior (Tanya does *not* die on C4 in the original game; SEAL
   does not either — confirm whether a "Suicide" flag exists and whether it's
   set).
5. **Animation cycle.** How are DoType `0x1b–0x1e` (Fire1–Fire4) wired to the
   CHARGE.SHP / CHARGEN.SHP planting animation? When does each frame advance?
   How long is the total animation? Where does the timing live?
6. **Cursor + targeting.** What action enum does
   `InfantryClass::What_Action_OnObject` return when a SEAL hovers a valid C4
   target, and how does that map to the on-screen cursor? What gates a
   building as "valid C4 target" (CanC4=yes alone? Or also iron-curtain check,
   on-bridge check, etc.)?
7. **Edge cases.** What happens when:
   - Target building is iron-curtained?
   - Target building is on a bridge?
   - SEAL is killed mid-plant (during DoType 0x1b–0x1e)?
   - Building is destroyed by another source mid-plant?
   - Two SEALs target the same building?
   - SEAL is force-fired (Ctrl-click) on a non-CanC4 building or on a unit?
   - Target is inside fog/shroud?

The doc must classify every finding as **Active in YR / TS-legacy / dormant**
(see CLAUDE.md "Tiberian Sun legacy code" section).

---

## 2. Prior Research Inventory

| Report | Relevant Scope | Confidence | Known Gaps |
|--------|---------------|------------|------------|
| [ENGINEER_CAPTURE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/ENGINEER_CAPTURE_GHIDRA_REPORT.md) §5 ("C4 / Sabotage") | C4 vs. capture distinction; says C4 flag at `InfantryType+0xEC2` | HIGH on capture, MEDIUM on C4 | Conflicts with Agent D's finding that C4 uses `+0xebe` (likely two distinct flags with overlapping semantics) |
| [FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md) §9 | `Mission_Capture` at `0x004D4B20` handles BOTH Capture (mission enum 8) AND Sabotage (enum 17) | HIGH on the function existing | Does NOT decompile the Sabotage branch internals |
| [FOOTCLASS_MISSION_ATTACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_ATTACK_GHIDRA_REPORT.md) §10 | Open question: TypeClass+0x695 unnamed flag; suspected C4/melee marker | MEDIUM | Resolution deferred — re-evaluate |
| [BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md) / `_V3.md` | `CanC4=` at `BuildingTypeClass+0x1577` | HIGH | — |
| [BUILDINGCLASS_UPDATE_AI_TICK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_UPDATE_AI_TICK_GHIDRA_REPORT.md) | References `Type+0x16A9` for CanC4 in garrison-fire path | MEDIUM | **Conflicts with V2/V3 (+0x1577)** — must reconcile |
| [MISSION_GUARD_AREAGUARD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_GUARD_AREAGUARD_GHIDRA_REPORT.md) §1 | Corrects prior mislabeling of "Assaulter" — says **C4= is at `InfantryType+0xEC2`** (gates AI auto-Sabotage) | HIGH | Conflicts with Agent D's `+0xebe` finding |
| [ANIMCLASS_CHAINING_DAMAGE_OWNERSHIP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_CHAINING_DAMAGE_OWNERSHIP.md) (RING1) | `RING1` animation uses hardcoded `Rules->C4Warhead` | HIGH | RING1 may be the post-detonation explosion anim spawned by `Apply_area_damage` — verify |
| [ANIM_CLASS_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_DEEP_DIVE.md) (RING1 path) | RING1 applies area damage with C4Warhead, 0-radius | HIGH | — |
| [WARHEAD_DETONATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEAD_DETONATE_GHIDRA_REPORT.md) | C4Warhead at `Rules+0xFAC` (note variant) | HIGH | **Conflicts with Agent D's `+0xfa8`** — likely a typo in one report; verify |
| [BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md) | C4Warhead used in forced-damage mode | HIGH | Doesn't trace the SEAL-side caller |
| [BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md) | EXPLICITLY notes BombClass is Crazy Ivan, NOT SEAL/Tanya C4 | HIGH | Useful as reference for what C4 is *not* |
| [READINI_FIELD_MAPS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/READINI_FIELD_MAPS.md) | INI key index | MEDIUM | Not yet checked for C4=/CanC4= entries — verify in execution |
| [MouseClass_research.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MouseClass_research.md) | Notes `IvanBomb=yes` flag at `InfTypeClass+0xEBE` | HIGH on Ivan | Likely confused with C4 in some prior reports — reconcile |

**Conflicts between reports — must be resolved during execution:**
1. **C4 flag offset on InfantryTypeClass** — three candidates surfaced:
   `+0xEBE` (Agent D + MouseClass), `+0xEC2` (MISSION_GUARD_AREAGUARD +
   ENGINEER_CAPTURE), `+0xEC8` (Agent D — possibly a *separate* sub-flag).
   Hypothesis: these are three different flags (Infiltrate-base, Agent/Spy,
   DemolishesByDoType), all of which can imply "is C4-capable" via the
   aggregation pattern Agent D observed in `InfantryTypeClass__ReadINI`.
   Execution must extract the exact PUSH order in the ReadINI assembly to
   confirm flag→name mapping.
2. **CanC4 offset on BuildingTypeClass** — `+0x1577` vs `+0x16A9`. Possibly
   one is the parsed flag and the other is a derived/cached form, OR one is
   stale doc text — reconcile.
3. **C4Warhead offset on RulesClass** — `+0xFA8` (Agent D, BUILDING_DAMAGE)
   vs `+0xFAC` (WARHEAD_DETONATE). Verify by reading RulesClass struct
   layout.

---

## 3. Function Inventory

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|-------|----------------|
| 1 | 1 | `0x005196a0` | `InfantryClass__Mission_Enter` | **The detonation site.** When mission==0x11 AND `+0xec2!=0` AND target == destination building, calls `Apply_area_damage(self, Rules->C4Warhead, 1, 0)` then triggers self-destruct via `vtable+0x1e8(2,0)`. Must extract: full conditional tree, exact damage args, vtable slot semantics, animation hook timing, the iVar2==2|3 final-block path. | FULL | LOW — clearly YR-active (SEAL/Tanya are YR units) |
| 2 | 1 | `0x0051f3e0` | `FUN_0051f3e0` (likely `InfantryClass__Mission_Attack`) | **The pre-dispatcher.** Branches Attack→{Engineer/Spy enter, C4 plant kickoff, mid-attack continuation, generic FootClass fallback}. Three flag tests: `+0xec2`/WeaponAbility 0xe → 0x11; `+0xebe`/(`+0xeb4|+0xeb5`+CanDock) → 8; DoType 0x1b–0x1e → return mid-attack. Decompile every branch + extract the iVar values that gate them. | FULL | LOW — but note flag-aggregation may mean some branches are TS-legacy gated |
| 3 | 1 | `0x00524400` | `InfantryTypeClass__ReadINI` | **Flag parser.** Resolves the disputed offsets (`+0xebe`, `+0xec2`, `+0xec3`, `+0xec4`, `+0xec6`, `+0xec8`, `+0xeb4`, `+0xeb5`). Read the PUSH order in the ReadBool calls — that's where the INI key string each offset binds to is visible. | FULL | MEDIUM — aggregation logic (`if any infiltration flag set, +0xebe = 1`) smells TS-legacy. Verify which flags are set on stock SEAL/Tanya/Engineer in rulesmd.ini |
| 4 | 1 | `0x004d4b20` | `Mission_Capture` (per FOOTCLASS_MISSION_HANDLERS) | **Mission state 8/17 dispatcher.** Per prior report, handles BOTH Capture (Engineer) AND Sabotage (C4). Need to find the C4-specific branch that Agent D didn't trace, and reconcile with Mission_Enter (#1). Possibly Mission_Capture sets up adjacency walk, then on arrival it transitions to Mission_Enter for the actual detonation. | FULL | LOW |
| 5 | 1 | `0x0066bbd1` | `RulesClass__ReadCombatDamage` (C4Warhead-reading region) | Reads `C4Warhead=` into `Rules+0xfa8` (or `+0xfac` per WARHEAD_DETONATE — reconcile). Also reads `C4Delay=` (double) into `Rules+0x1750`. Extract exact offsets and parser type. | MEDIUM | LOW |
| 6 | 1 | `0x00460050` | `BuildingTypeClass_ReadINI_Water` (CanC4 region) | Reads `CanC4=` into `BuildingType+0x1577`. Reconcile with `+0x16A9` claim from BUILDINGCLASS_UPDATE_AI_TICK. Note where `CanC4=no` is set on hardcoded buildings (CAMISC01/02, CAMSC09/10). | MEDIUM | LOW |
| 7 | 2 | `0x0051d6f0` | `InfantryClass__Do_Action` | DoType setter — supports DoType 0x1b–0x1e (Fire1–Fire4) which carry the C4 plant animation. Sets `+0x6db` flag on action 5. Uses `Rules+0x16c0` health threshold (what for?). Map every DoType code path to the specific animation/sequence it triggers. | MEDIUM | LOW |
| 8 | 2 | `0x00520ae0` | `InfantryClass__DoType_Sequencer` | Per-frame animation tick. Case 0x1b advances to 0x1c, calls `FUN_0070f770` (likely SetTarget). On end of 0x1b/0x1c/0x1d/0x1e transitions back to 0x1c (loop continuation). Extract exact frame counts and the looping-vs-terminal logic — this is the timing source for the planting animation. | FULL | LOW |
| 9 | 2 | `0x0051df70` | `InfantryClass__Fire_At_Override` | Override of TechnoClass::Fire_At. Tests `+0xebf` (Suicide-on-fire?) and mission ∈ {1,0xf} → calls vtable+0x1e8 (self-destruct). Likely the C4 self-immolation path — but the original game does NOT kill the SEAL on C4, so this may be either dormant or only fired in specific edge cases. **Critical to verify whether this fires for SEAL/Tanya in stock YR.** | FULL | MEDIUM — possibly TS Demolition-Truck legacy that aliases here |
| 10 | 2 | `0x005206b0` | `InfantryClass__Fire_At_Target` | Walks DoType cycle to firing-frame; calls vtable+0x3cc (FireWeapon) when frame matches. Sequences 0x1b–0x1e are the C4 firing frames. Map: which frame number triggers the actual `Apply_area_damage` call? | FULL | LOW |
| 11 | 2 | `0x0051e3b0` | `InfantryClass__What_Action_OnObject` | Cursor-action picker. `+0xebe` true + iVar7==5 returns 0x10 (DEMOLISH cursor) when target has `Type+0x1577 (CanC4)`. Also branches: `+0xec3` → 0x39 (capture), `+0xeae` → 0x35/0x36, `+0xec8` → 0x40/0x47. Extract every action enum value and what it maps to in cursor space. | FULL | LOW |
| 12 | 2 | `0x00489280` | `Apply_area_damage` | Wraps `WarheadTypeClass::Detonate`. Light pass — already documented elsewhere; we just need to confirm the exact entry signature and how the C4Warhead flows into building damage. Cross-reference with [WARHEAD_DETONATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEAD_DETONATE_GHIDRA_REPORT.md). | LIGHT | LOW |
| 13 | 3 | `0x004d4dc0` | `FootClass__Mission_Attack` | Generic fallback called by #2 after special branches don't match. Issues vtable+0x484/0x53c walk-toward-target. We need this to confirm the "infantry walks to building" path is the same one as for normal attacks. | LIGHT | LOW |
| 14 | 3 | `0x006fdd50` | `TechnoClass::Fire_At` | Ancestor of Fire_At_Override. Generic weapon-fire dispatch. We need its call signature and how the C4 weapon (Sapper) flows through, but not its full body. | LIGHT | LOW |
| 15 | 3 | (vtable lookup) | `TActionClass[mission_id]` dispatch table | The mission-state → mission-handler indirect. Confirm slot 0x11 (Mission_Enter) and slot 8/17 (Mission_Capture) point at #1 and #4. Just verify the dispatch — no decompile needed. | LIGHT | LOW |
| 16 | 3 | (string xref) | RING1 anim spawn site | Per ANIMCLASS docs, RING1 is spawned with C4Warhead. Find the spawn caller and confirm whether RING1 is the post-C4 explosion animation or something else. May or may not be in the SEAL path. | LIGHT | LOW |
| 17 | 2 | `0x00701900` (region) | `TechnoClass::ReceiveDamage` (C4-relevant branch) | The receiving end on the building side. Verify whether C4 damage takes a special path (e.g., immediate destruction regardless of Strength) or just deals 100% via Mechanical/Super warhead and lets normal damage math kill the building. Light skim only — full RECEIVE_DAMAGE doc already exists. | LIGHT | LOW |

**Total: 17 functions** (5 Phase-1 core, 7 Phase-2 depth, 5 Phase-3 context).
Sits in the 8–30 normal-plan band.

**Phase 1 checkpoint:** After Phase 1 (#1–#6), the executor must produce a
findings summary covering: confirmed flag offsets (resolving the `+0xEBE` vs
`+0xEC2` conflict), the Mission_Enter↔Mission_Capture relationship, the
Rules+0xfa8/0xfac C4Warhead offset, and a one-paragraph end-to-end pipeline
sketch. If Phase 1 reveals the scope is wrong (e.g., `Mission_Enter` turns
out to also serve Engineer Capture and the C4 path is somewhere else), revise
the plan before Phase 2.

---

## 4. Detail Checklist

### Magic numbers and offsets to extract
- `Rules+0xfa8` (or `+0xfac`?) → `C4Warhead` (string ref `0x0083b1d4`)
- `Rules+0x1750` → `C4Delay` (double)
- `Rules+0x16c0` → health threshold used in `Do_Action` — what for?
- `BuildingType+0x1577` (or `+0x16A9`?) → `CanC4`
- `InfantryType+0xebe`, `+0xec2`, `+0xec3`, `+0xec4`, `+0xec6`, `+0xec8`,
  `+0xeb4`, `+0xeb5`, `+0xebf` → flag name for each (read the PUSH order in
  ReadINI)
- `+0x6db` (InfantryClass instance) → set on action 5; verify what reads it
- DoType codes `0x1b`, `0x1c`, `0x1d`, `0x1e` → Fire1–Fire4; map to art.ini
  Sequence entries
- Mission state codes `0x11` (Enter), `8` (Capture), `17` (Sabotage) →
  confirm enum values match [MISSIONCLASS_STATE_MACHINE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSIONCLASS_STATE_MACHINE.md)
- Action enum values `0x10`, `0x35`, `0x36`, `0x39`, `0x40`, `0x47` → cursor
  IDs from MouseClass

### Bit flags and masks
- WeaponAbility bit `0xe` (used in branch 1 of `FUN_0051f3e0`) — what enum?
- Any `& 0xFF` / `| 0x40` patterns in `InfantryTypeClass__ReadINI` flag
  aggregation
- Mission-state bitfield, if any

### State machine states
- Infantry mission enum: which states does the C4 path traverse? Likely
  `Move` → `Sabotage` (or `Capture` for the dispatch shared with Engineer) →
  `Enter` → terminal. Map every transition.
- DoType cycle: `0x1b` → `0x1c` → `0x1d` → `0x1e` → loop or terminate?
  Sequencer (#8) decides — extract.

### INI keys to verify (full list in §5)
Every key in §5 must be confirmed parsed at the correct offset and confirmed
*read* somewhere in the C4 pipeline (no orphan keys).

### Struct offsets to extract
- `InfantryTypeClass`: 9 flag offsets in the `+0xeae..+0xec8` band (note
  `param_1` type — `int` per `TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md`, so
  byte-offset reads are direct)
- `BuildingTypeClass`: `CanC4` at `+0x1577` (verify)
- `RulesClass`: `C4Warhead` ptr, `C4Delay` double — verify field types
- `InfantryClass`: `+0x6db` action-5 flag; any other instance fields written
  during a C4 plant
- `WeaponTypeClass`: ability bit `0xe` source field

### Clamps, rounding, off-by-ones
- C4Delay: frames or seconds? If frames, does the C4 fire on frame N or
  frame N+1?
- Animation looping: does DoType_Sequencer (#8) re-enter 0x1c forever until
  fire frame, or is there an exit count?
- Adjacency check: cell-distance == 1 (cardinal+diagonal) vs cell-distance
  ≤ √2 (radial)?

### Edge cases to test
- Iron Curtain on target: blocks C4? Cancels mid-plant?
- Target on bridge: C4 the building only, or also the bridge tile?
- SEAL killed mid-plant: aborts cleanly?
- Two SEALs same target: first wins, both fire, conflict?
- Force-fire on non-CanC4 building: rejected by What_Action, or accepted then
  no-op'd?
- Force-fire on a unit (not building): falls through to Sapper damage via
  `Mechanical` warhead? (Sapper Damage=2500 on a unit would matter.)
- Fog/shroud on target: cursor still C4, or "?" cursor?

### Timing/ordering
- Where does the C4 plant fit in `World::advance_tick`?
  - Movement (walk to building) → ground movement phase
  - Mission state transition Sabotage → Enter → mission dispatch phase
  - DoType animation tick → infantry update phase (which sub-system?)
  - `Apply_area_damage` call → combat phase
  - Building destruction → damage/destruction phase
- Confirm: does the entire C4 plant happen in a single tick, or spread over
  N ticks (the animation cycle)?

### TS-legacy flags (consolidated in §7)
- `SpecialFlags & ?` — any C4-related gate? Agent D didn't see one but
  re-check.
- The flag-aggregation in `InfantryTypeClass__ReadINI` (any infiltration role
  → `+0xebe`=1) smells like TS-legacy unification of multiple TS infantry
  roles. Verify whether stock YR sets multiple of these on any single unit.

### Vtable dispatches to resolve
- `vtable+0x1e8(2,0)` from #1 — likely Self_Destruct
- `vtable+0x1e8(0x11,0)` from #2 — Set Mission(Enter)?
- `vtable+0x1f0(8)` from #2 — Set Mission(Capture)?
- `vtable+0x16c` from #1 — Take_Damage on building (via base ObjectClass
  vtable?)
- `vtable+0x3cc` from #10 — FireWeapon
- `vtable+0x484` / `vtable+0x53c` from #13 — walk-toward-target

Resolve each by walking the appropriate vtable from base class addresses
already documented in [TECHNOCLASS_VTABLE_COMPLETE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_VTABLE_COMPLETE.md) /
[FOOTCLASS_VTABLE_COMPLETE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_VTABLE_COMPLETE.md).

---

## 5. INI Keys in Scope

### A. Per-infantry C4 demolitionist flags

| Key | Section examples | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|------------------|---------|-------------------|----------------------------|
| `C4` | `[SEAL]=yes`, `[TANY]=yes`, `[VIRUS]=yes`, `[CHRONO]=;yes`, `[STALKER]=;yes` | `no` | "This unit can plant C4 / sabotage buildings" | **No** — not parsed at all (only `Assaulter` is parsed in `object_type.rs`) |
| `Assaulter` | `[SEAL]=no`, `[TANY]=no`, `[GIRT]=yes`? | `no` | "This unit can clear out under-construction buildings" — RELATED but DIFFERENT mechanic from C4. Parsed at `object_type.rs:503-504` | **Yes** — parsed but not used for C4 |
| `IsDemolitionist` | (verify presence) | `no` | TS-legacy candidate? Verify if any stock infantry sets it. | No |
| `DestroysBuildings` | (verify presence) | `no` | TS-legacy candidate? | No |

### B. Per-building C4 eligibility

| Key | Section examples | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|------------------|---------|-------------------|----------------------------|
| `CanC4` | `[CAMISC01]=no` (Oil Derrick), `[CAMISC02]=no` (Barrel), `[CAMSC09]=no`, `[CAMSC10]=no` (McBurger Kong) | `yes` | "This building is a valid C4 target" — non-yes overrides default | **No** |

### C. Global combat damage rules

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|---------|-------------------|----------------------------|
| `C4Warhead` | `[CombatDamage]` | `Super` | Warhead used for C4 damage application | **Partial** — parsed in `bridge_warheads.rs` for bridge collapse only; not exposed for infantry C4 |
| `C4Delay` | `[CombatDamage]` | (verify default) | Frames between plant start and detonation? Or animation length? | **No** |
| `IvanWarhead` | `[CombatDamage]` | `IvanWH` | Crazy Ivan's warhead — adjacent, NOT for C4 | (separate system) |

### D. Weapons referenced by C4 units

| Section | Key fields | Notes | Currently Parsed? |
|---------|-----------|-------|-------------------|
| `[Sapper]` (rulesmd:22846) | `Damage=2500`, `ROF=100`, `Range=1.5`, `CellRangefinding=yes`, `Projectile=Invisible`, `Warhead=Mechanical`, `Report=SealPlaceBomb` | SEAL/Tanya secondary weapon — fires the C4 hit | WeaponType parser handles it generically; but the special "fire as C4 plant" semantics are not implemented |
| `[FakeC4]` (rulesmd:23065) | `Damage=5000`, `ROF=10`, `Range=1.5`, `CellRangefinding=yes`, `Projectile=InvisibleLow`, `Warhead=FakeC4WH`, `Report=SealPlaceBomb`, `SabotageCursor=yes` | Chrono Commando's fake C4 — the unit's `C4=` is commented out, so this fires through the normal weapon path with a Yuri-only warhead | Generic weapon parse only |
| `SabotageCursor` (per-weapon) | bool | "Show sabotage cursor instead of fire cursor on this weapon's target" | **Yes** — `weapon_type.rs:126-127, 250` |

### E. Warheads

| Section | Key fields | Notes | Currently Parsed? |
|---------|-----------|-------|-------------------|
| `[Super]` (rulesmd:27093) | `Verses=100%×11`, `AnimList=XGRYSML1,...,TWLT070`, `InfDeath=2` | Default `C4Warhead=Super` — kills everything | Yes (generic) |
| `[Mechanical]` (rulesmd:27116) | `Verses=0%,0%,0%,100%,100%,100%,0%,0%,0%,100%,100%` | Sapper weapon's warhead — only damages mechanical (buildings, vehicles) | Yes (generic) |
| `[FakeC4WH]` (rulesmd:26952) | `CellSpread=0`, `Verses=0,0,0,0,0,100%,100%,100%,0,100%` | Chrono fake C4 — restricted | Yes (generic) |

### F. Audio / voice

| Key | Where | Value | Currently Parsed? |
|-----|-------|-------|-------------------|
| `VoiceSpecialAttack` | `[SEAL]:4048` | `SealSpecialAttack` | Generic VoiceSpecialAttack parse exists; verify it fires on C4 plant |
| `Report` | `[Sapper]`, `[FakeC4]` | `SealPlaceBomb` | Generic weapon-Report parse exists |

### G. Animation

| Key | Where | Value | Currently Parsed? |
|-----|-------|-------|-------------------|
| Sequence `Fire1`/`Fire2`/`Fire3`/`Fire4` | `[E7Sequence]` (Tanya), `[NaSEALSequence]` etc. in artmd.ini | DoType 0x1b/0x1c/0x1d/0x1e frame ranges | Generic Sequence parser handles it; verify the C4 plant runs through Fire1–Fire4 specifically |
| `CHARGE.SHP` / `CHARGEN.SHP` | (likely an Image= or AnimList ref) | C4 placement animation | **Verify in execution** — Agent D could not find these as string literals; they may be Image= references on a sequence definition |

---

## 6. Caller & Integration Map

### Binary callers (gamemd)

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|-----------|--------------|----------------------------|
| TActionClass mission-dispatch table | #1 (Mission_Enter @ `0x005196a0`) | Every tick when infantry has mission state == 0x11 | LIGHT — confirm the dispatch slot |
| TActionClass mission-dispatch table | #4 (Mission_Capture @ `0x004d4b20`) | Every tick when infantry has mission state == 8 or 17 | LIGHT |
| #2 (Mission_Attack @ `0x0051f3e0`) | #1, #4, #13 | Per-tick from FootClass::Mission_Attack base, when infantry's Mission == Attack | YES — already in Phase 1 |
| `InfantryClass::AI_Update` (in TechnoClass::AI_Update) | #2 | Per tick, on every infantry | LIGHT — already in TECHNOCLASS_SYSTEMS doc |
| `BuildingClass::ReceiveDamage` | (incoming side) | When `Apply_area_damage` from #1 hits | LIGHT — RECEIVE_DAMAGE_PIPELINE doc covers this |
| `MouseClass::Get_Action` / `DisplayClass::Get_Cursor` | #11 | On every mouse-hover | YES — already in Phase 2 |
| Player input (right-click / left-click on building with SEAL/Tanya selected) | sets infantry's NavCom/TarCom and triggers Mission_Attack | Once per click | NO — generic player-input plumbing already documented |

### Rust integration today

| Rust file | What it does | What it would need for C4 |
|-----------|--------------|---------------------------|
| `src/rules/object_type.rs` | Parses InfantryType including `Assaulter`, `SabotageCursor` | Add `c4: bool` parsed from `C4=`; add the related Engineer/Spy flags if they end up sharing parser logic |
| `src/rules/building_type.rs` (or wherever BuildingType lives) | Parses BuildingType | Add `can_c4: bool` parsed from `CanC4=` (default `yes`) |
| `src/rules/ruleset.rs` | Holds `C4Warhead` (currently for bridges only) | Expose `c4_warhead_id` for infantry C4 path too |
| `src/rules/weapon_type.rs` | Has `sabotage_cursor` bool | No change needed |
| `src/app_cursor.rs:154,214-219` | Shows Enter cursor on EnemyStructure when SabotageCursor weapon | Either keep this as-is (Enter cursor is good enough) OR add a dedicated C4 cursor variant; gate display on `c4` flag + `can_c4` target flag |
| `src/sim/combat/` (no current C4 logic) | Generic weapon fire | Add C4-attack mode: when an `c4=true` infantry attacks a `can_c4=true` building, branch into walk-up + plant + detonate flow |
| `src/sim/world/world.rs::advance_tick` | Tick orchestrator | Add C4-plant ticking somewhere — likely in the existing turret/combat or mission-dispatch slice |
| `src/sim/animations/` | Animation system | Wire `Fire1`–`Fire4` sequences to play on the SEAL during plant; spawn post-detonation explosion anim (RING1?) |
| `src/audio/` | Sound system | Hook `Report=SealPlaceBomb` on plant start, `VoiceSpecialAttack` on selection |

### Callers we will NOT investigate

- AI script-action callers (TaskForce, ScriptType, TeamType) — separate
  AI subsystem; deferred per `feedback_no_ai_yet.md`. The investigation only
  covers the player-issued path.
- Multiplayer event encoding for the plant — assumed identical to a normal
  attack-event packet; verify by skimming `EventClass::Execute` only if the
  player-input investigation surprises us.

---

## 7. TS-Legacy Risk Register

| Risk | Where it surfaces | Verification step |
|------|-------------------|---------------------|
| **Flag-aggregation in `InfantryTypeClass__ReadINI`** — Agent D observed that any of `+0xec2`/`+0xec3`/`+0xec4` being set forces `+0xebe = 1`. This unification of "Infiltrate / Engineer / Agent → all imply C4-capable" smells TS-legacy. | #3 (`InfantryTypeClass__ReadINI`) | Read the assembly to confirm the aggregation, then check rulesmd.ini: does any stock infantry actually have multiple of these flags set? If only one is ever true at a time, the aggregation is just defensive plumbing; if Engineer accidentally gets `+0xebe=1`, that would imply Engineer is C4-capable too — verify. |
| **`InfantryClass__Fire_At_Override` self-destruct path** — The original game does NOT kill SEAL or Tanya on a C4 plant. But this function self-destructs when `+0xebf != 0` and mission ∈ {1,0xf}. Either `+0xebf` is never set on SEAL/Tanya, or this is a TS Demolition-Truck legacy that aliases here. | #9 (`Fire_At_Override`) | Trace `+0xebf` writes and confirm it's `Suicide=yes` (and verify which units set it — likely Terrorist and Demolition-Truck-equivalent only). |
| **TypeClass+0x695 unnamed flag** — Per FOOTCLASS_MISSION_ATTACK §10, this is suspected to be C4/melee marker but unverified. May be TS-era. | Not in this plan's function inventory directly — but if it surfaces during execution, add it. | If found relevant, add a function to the inventory and recurse. |
| **`Mission_Capture` handles BOTH Capture (8) and Sabotage (17)** — this is YR-active (Engineer + SEAL/Tanya are YR). But the handler may contain TS-era branches (e.g., Demolition-Truck-style instant-destruction without an animation). | #4 (`Mission_Capture`) | When decompiling, classify each branch by which infantry/mission triggers it. |
| **`SpecialFlags` gate on any C4 path** | Agent D did not see one but did not exhaustively check. | During Phase 2, run a quick byte-pattern check in #1, #2, #4, #11 for `SpecialFlags` reads (typically `MOV ?, [DAT_???? + 4]`). Note any hits. |
| **RING1 anim — TS-era?** | #16 | RING1 is referenced by C4Warhead per ANIMCLASS docs. If RING1 is in YR's animlist, fine; if it's TS-only, the SEAL plant may use a different post-detonation anim in YR. Cross-check artmd.ini. |

---

## 8. Current Rust Implementation Surface

| Path | Lines | What's covered | Gap |
|------|-------|----------------|-----|
| `src/rules/bridge_warheads.rs` | 7, 27–29, 36, 48 | Parses `C4Warhead=` for bridge collapse only; default `"Super"` | Not exposed for the infantry C4 path |
| `src/rules/ruleset.rs` | 1183, 1394, 1426, 1444–1445, 2736, 2740, 2755 | Interns `C4Warhead` name; one consumer (bridge orchestrator) | Add a second consumer — the infantry C4 attack — using the same interned id |
| `src/rules/object_type.rs` | 503–504, 892, 923 | Parses `Assaulter=` and (via weapon_type) `SabotageCursor=` | Add `c4: bool` (and possibly `agent`, `engineer`, `infiltrator` if the parser shares logic with Spy/Engineer flags) |
| `src/rules/weapon_type.rs` | 126–127, 250 | Parses `SabotageCursor=yes` per-weapon | OK as-is |
| `src/app_cursor.rs` | 154, 214–219 | Shows `CursorFeedbackKind::Enter` on EnemyStructure when selected unit's weapon has `sabotage_cursor=true` | Add gating: must be `c4=true` infantry hovering a `can_c4=true` building. Currently any `SabotageCursor` weapon shows the cursor regardless. |
| `src/app_types.rs` | 175 | `CursorFeedbackKind::Enter` doc-comment lists "garrison, capture, board transport, sabotage" | Confirm a single Enter cursor is the right choice; the original game may use a distinct demolish cursor (action enum 0x10 per #11). Investigation must extract the cursor SHP frame mapping. |
| `src/sim/combat/combat_weapon.rs` | 1–99+ | Generic Primary/Secondary selection by Verses/AA/AG | Add C4-attack mode branch: if attacker is `c4` and target is a building with `can_c4`, use the C4 plant flow instead of normal weapon fire |
| `src/sim/world/bridge_orchestrator.rs` | 105–111 | Bridge-occupant force-kill using C4Warhead's InfDeath byte | OK as-is — this is a different consumer of the same `C4Warhead` name |
| `src/sim/world/world_tests.rs` | 836 | Bridge-collapse test mentions C4Warhead | Will need new tests for SEAL/Tanya C4 attack once implementation lands |
| `src/sim/deploy.rs` | 27–43, 57–85 | Deploy phase state machine | Not relevant to C4 |

**Greenfield areas:**
- Per-infantry `c4: bool` flag (rules + struct + parser test)
- Per-building `can_c4: bool` flag (rules + struct + parser test)
- C4-attack mode in combat_weapon.rs / mission-dispatch
- Walk-up adjacency detection (or reuse of existing footprint adjacency for
  Engineer/Spy enter)
- Plant animation cycle (DoType 0x1b–0x1e equivalent)
- `Apply_area_damage`-equivalent call on plant completion
- Plant-cancellation paths (death mid-plant, target destroyed, iron curtain)

---

## 9. Deferred Open Questions

These were surfaced during scoping but not resolved — the executor must
answer them or explicitly re-document as unresolved.

1. **Three flag offsets, three semantics:** which of `+0xebe`, `+0xec2`,
   `+0xec8` is the "C4=" parsed flag, and what are the other two? The
   working hypothesis from Agent D is: `+0xebe` = "Infiltrate-base
   (anyone-can-enter-buildings)", `+0xec2` = "Agent/Spy", `+0xec8` =
   "DemolishesByDoType" (the actual C4 mechanic). Resolve by reading the
   PUSH order in `InfantryTypeClass__ReadINI` (#3).
2. **CanC4 offset:** `+0x1577` (V2/V3 master) or `+0x16A9`
   (UPDATE_AI_TICK)? Resolve by reading the
   `BuildingTypeClass_ReadINI_Water` ReadBool call site directly (#6).
3. **C4Warhead offset:** `Rules+0xfa8` or `Rules+0xfac`? Resolve by reading
   the `RulesClass__ReadCombatDamage` ReadString call site directly (#5).
4. **C4Delay semantics:** frames vs seconds? Used where? Resolve by tracing
   `Rules+0x1750` reads.
5. **Self-destruct on plant — does it fire for SEAL/Tanya in stock YR?**
   Original game says no. Resolve by tracing `+0xebf` writes and confirming
   it's `Suicide=yes` only on Terrorists / Demo-Truck-equivalents.
6. **DoType animation cycle exact length:** how many frames does a full C4
   plant take from start to detonation? Required for parity. Resolve in #8.
7. **Adjacency rule:** does the SEAL plant from the same cell as the
   building (impossible for non-passable foundations) or from any adjacent
   cell (cardinal? diagonal too?)? Resolve in #1, #4, #13.
8. **Force-fire (Ctrl-click) on a unit:** does Sapper fire as a normal
   weapon and deal 2500 damage via Mechanical warhead, or is the C4 path
   still entered? Resolve in #2 + #11 cursor logic.
9. **Iron Curtain interaction:** does C4 detonate-on-plant ignore IC, or
   does the IC absorb it? Resolve in the damage-pipeline trace (#1 + #12).
10. **Mission_Capture (#4) vs Mission_Enter (#1) handoff:** are they
    sequential (Capture sets up, Enter detonates), or do they live in
    parallel mission slots (8 vs 17 vs 0x11)? Resolve in Phase 1.
11. **RING1 anim:** is this the post-C4 explosion or a different effect?
    Resolve in #16.
12. **TypeClass+0x695 from FOOTCLASS_MISSION_ATTACK §10:** still suspected
    C4-related — confirm or rule out.

---

## 10. Execution Strategy

**Recommendation: Single-session `/re-investigate`** with mid-session
checkpoint after Phase 1.

Rationale:
- 17 functions is mid-range; one focused session can cover them.
- Phase 1 results (5 entry points + 2 ReadINI sites) need to be settled
  before Phase 2 because the offset reconciliations gate everything else.
  Splitting Phase 1 across sessions risks losing context.
- Phase 2 (animation/firing/damage helpers) and Phase 3 (callers/dispatch)
  are mostly independent and can be done in parallel sub-passes within the
  same session.

**Concrete execution order:**
1. Open Ghidra MCP, decompile #5 (RulesClass) and #6 (BuildingType ReadINI)
   first — they're the smallest and resolve two of the offset conflicts.
2. Decompile #3 (InfantryType ReadINI) — resolves the flag offsets. This is
   the highest-leverage step.
3. Decompile #1 (Mission_Enter), #2 (Mission_Attack), #4 (Mission_Capture)
   together — they form the dispatch triangle. Cross-reference between
   them.
4. **Phase 1 checkpoint** — produce a 2-page interim summary covering
   confirmed offsets, dispatch graph, and the pipeline sketch. Verify with
   the user before Phase 2.
5. Phase 2 — DoType cycle (#7, #8, #10), Fire_At_Override (#9), What_Action
   (#11), Apply_area_damage skim (#12), ReceiveDamage skim (#17).
6. Phase 3 — caller confirmations (#13, #14, #15), RING1 anim (#16).
7. Write final report to [NAVY_SEAL_TANYA_C4_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/NAVY_SEAL_TANYA_C4_GHIDRA_REPORT.md).

If Phase 1 reveals the function inventory is wrong — e.g., Mission_Capture
(#4) turns out to be irrelevant and the actual Sabotage handler is
elsewhere — return to this plan and revise before continuing.

---

## 11. Success Criteria

The executed research document must:

- [ ] Answer every question in §1 (the seven goal questions).
- [ ] Include every function from §3 with its address, final name, and a
      decompiled-to-target-depth analysis. Any omission must be justified
      (e.g., "Function #16 turned out to be unused in YR — verified by xref
      check").
- [ ] Resolve the three offset conflicts in §2:
      - `InfantryType` C4 flag: `+0xebe` vs `+0xec2` vs `+0xec8`
      - `BuildingType` CanC4: `+0x1577` vs `+0x16A9`
      - `Rules` C4Warhead: `+0xfa8` vs `+0xfac`
- [ ] Resolve every deferred question in §9 or explicitly document them as
      unresolved with the reason.
- [ ] State **"Active in YR: Yes / No / Conditional"** for every finding,
      with the trigger frequency for "Conditional" (per CLAUDE.md severity
      rule).
- [ ] Cite Ghidra addresses for every HIGH-confidence claim.
- [ ] Document the exact end-to-end pipeline: player-click → mission state
      → animation → damage → cleanup, with frame-precise timing where
      visible.
- [ ] Confirm CHARGE.SHP / CHARGEN.SHP usage: is it a literal Image=
      reference somewhere in artmd.ini, or is it a sequence frame range
      that the SHP loader resolves from the unit's main SHP?
- [ ] Provide an INI reference table with verified parsed offsets for every
      key in §5.

---

## Sources

**Ghidra addresses sampled during scoping:**
`0x005196a0`, `0x0051f3e0`, `0x00524400`, `0x004d4b20`, `0x0066bbd1`,
`0x00460050`, `0x0051d6f0`, `0x00520ae0`, `0x0051df70`, `0x005206b0`,
`0x0051e3b0`, `0x00489280`, `0x004d4dc0`, `0x006fdd50`, `0x00701900`,
plus xrefs from string `"CHARGE"`, `"CHARGEN"`, `"C4Warhead"`,
`"DestroysBuildings"`, `"CanC4"`, `"Assaulter"`.

**Docs searched (`docs/research/`):**
[ENGINEER_CAPTURE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/ENGINEER_CAPTURE_GHIDRA_REPORT.md),
[FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_HANDLERS_GHIDRA_REPORT.md),
[FOOTCLASS_MISSION_ATTACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_ATTACK_GHIDRA_REPORT.md),
[BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MASTER_GHIDRA_REPORT_V2.md),
[BUILDINGCLASS_MASTER_GHIDRA_REPORT_V3.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_MASTER_GHIDRA_REPORT_V3.md),
[BUILDINGCLASS_UPDATE_AI_TICK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_UPDATE_AI_TICK_GHIDRA_REPORT.md),
[MISSION_GUARD_AREAGUARD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MISSION_GUARD_AREAGUARD_GHIDRA_REPORT.md),
[ANIMCLASS_CHAINING_DAMAGE_OWNERSHIP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIMCLASS_CHAINING_DAMAGE_OWNERSHIP.md),
[ANIM_CLASS_DEEP_DIVE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_DEEP_DIVE.md),
[WARHEAD_DETONATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEAD_DETONATE_GHIDRA_REPORT.md),
[BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDING_DAMAGE_DESTRUCTION_GHIDRA_REPORT.md),
[BOMB_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BOMB_CLASS_GHIDRA_REPORT.md),
[READINI_FIELD_MAPS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/READINI_FIELD_MAPS.md),
[MouseClass_research.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MouseClass_research.md).

**INI files checked:**
`ini/rulesmd.ini` (sections `[SEAL]`, `[TANY]`, `[VIRUS]`, `[CHRONO]`,
`[STALKER]`, `[Sapper]`, `[FakeC4]`, `[Super]`, `[Mechanical]`, `[FakeC4WH]`,
`[CombatDamage]`, `[CAMISC01/02]`, `[CAMSC09/10]`).

**Related plans:**
`docs/plans/2026-05-05-particles-c4-fire-plan.md` (UNRELATED — covers
particle-system fire VFX, not C4 demolition mechanic).
