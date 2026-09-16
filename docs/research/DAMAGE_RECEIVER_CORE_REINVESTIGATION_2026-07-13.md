# Damage Receiver Core Reinvestigation Synthesis - 2026-07-13

Date: 2026-07-13  
Mode: bounded `/re-swarm` reconciliation of Tasks 1A, 1B, and 1C  
Parity target: active Yuri's Revenge `gamemd.exe`  
Synthesis status: **COMPLETE**  
Authority Gate G1: **FAILED**

## 1. Preflight, scope, and authority

This document reconciles exactly these three child reports:

- [DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_TECHNO_GATES_REINVESTIGATION_2026-07-13.md) (PARTIAL);
- [DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_OBJECT_CALLBACKS_REINVESTIGATION_2026-07-13.md) (COMPLETE for its bounded slice);
- [DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY_REINVESTIGATION_2026-07-13.md) (PARTIAL, G1 FAILED).

The output path did not exist at preflight. No same-file conflict was present. No Rust, INI, plan, claim ledger, child report, or older research document was changed. No Cargo command, debugger, Ghidra mutation, UI automation, or process/screen manipulation was used.

Authority is ranked as follows:

1. active `gamemd.exe` body/assembly and active-YR reachability;
2. retail `rules.ini` plus `rulesmd.ini` overlay and concrete retail assets where applicable;
3. fresh child-report binary evidence with inline call/address provenance;
4. older research only where it agrees with stronger evidence;
5. current Rust only as evidence of implementation state, never as native authority.

Two narrow read-only Ghidra checks were added during reconciliation because older documents contradicted a load-bearing child claim:

- `decompile_function(0x0050BD30)` and `disassemble_function(0x0050BD30)`, plus caller assembly at `0x00701945..0x00701952` and parser xrefs at `0x00511AD7..0x00511B53`, resolve the category-armor mapping.
- `get_assembly_context(0x007154A3)`, `get_assembly_context(0x007154E8)`, and `decompile_function(0x00477640)` resolve the Elite ability base and whole-array replacement behavior.

The reconciliation itself is complete: conflicts are decided, evidence strengths are retained, and every G1 row is classified. G1 nevertheless fails because authority-critical native behavior and mutable-field provenance remain open. This report is an implementation-contract input, not a parity certificate and not permission to cut the shadow receiver into the live path.

## 2. Executive verdict

The verified core is an ordered, signed, mutable damage transaction, not a pure `damage -> HP delta` calculation.

- `TechnoClass::ReceiveDamage @ 0x00701900` owns sign capture, defender modifiers, status gates, readiness mutation, bunker routing, warhead immunities, Psychedelic state, delegation, and a common postlude.
- `ObjectClass::ReceiveDamage @ 0x005F5390` owns the signed HP transaction, `Immune` and `CanC4` rules, healing, threshold result, one-time Cyborg survival, synchronous receiver triggers, fresh liveness/HP rereads, kill-credit selection, destruction, and survivor refresh.
- The mutable `int32 pDamage` is observable. Different early exits leave it unchanged, leave a transformed value, write zero, write a kernel result, or cap it to entry HP.
- Trigger actions execute synchronously inside the receiver. They can detach the tag, sell a Building, cause nested damage, rescue or kill the receiver, and change later fresh reads.
- The current live Rust path does none of this transactionally: it queues `u16` damage and later performs `u16::saturating_sub` before a deferred death pass.

Task 1B closes its bounded Object receiver slice. Tasks 1A and 1C do not meet the exhaustive authority bar. Therefore the combined G1 verdict is **FAILED** even though many high-value mechanisms are now assembly-verified.

## 3. Unified ordered native state machine

### 3.1 Effective call contract

Both receiver bodies consume seven 32-bit stack arguments and `RET 0x1C`. The verified effective Techno contract is:

```text
TechnoClass::ReceiveDamage(
    int32* pDamage,
    int32 distanceLeptons,
    WarheadTypeClass* warhead,
    ObjectClass* attacker,
    bool ignoreDefenses,
    bool arg6_unknown,
    HouseClass* sourceHouse)
```

At `0x00701DCC..0x00701DF8`, Techno passes all seven values unchanged and in the same order to Object. Object does not consume slot `+0x18`; the pass-through identity is verified, but the semantic name of `arg6_unknown` remains UNKNOWN (`DAMAGE_RECEIVER_TECHNO_GATES...:70-91`; `DAMAGE_RECEIVER_OBJECT_CALLBACKS...:54-68`).

All `Health`, `Strength`, `pDamage`, distance, and readiness-ammo operations in this slice are signed 32-bit unless a row explicitly names float/double storage.

### 3.2 Techno receiver prefix and delegation

| Order | Native predicate/action | `pDamage` / return behavior | Same-call side effects and evidence |
|---:|---|---|---|
| T1 | Snapshot `originalNegative = (*pDamage < 0)` | No write | Snapshot remains authoritative after later transforms; `0x00701910..0x00701927`. |
| T2 | If `!ignoreDefenses && !originalNegative`, divide by `(House/category armor * Techno+0x158)` and perform one `ftol` | Replaces `*pDamage` | Includes incoming zero; no zero-divisor guard. Grouping verified at `0x00701945..0x0070196C`. |
| T3 | Rank-selected `STRONGER` ability permits separate divide by `Rules+0x688 VeteranArmor` | Replaces `*pDamage` after a second `ftol` | Veteran array `+0x29C`; Elite array `+0x2AE`; selected byte index 1. |
| T4 | Same outer predicate: `if *pDamage < 1` | Writes 1 | Incoming zero becomes 1 when defenses are active. |
| T5 | `TypeImmune`: attacker nonnull, flag true, exact type pointer equal, exact owner pointer equal | Return 0 **without another write** | Caller sees the transformed integer, not zero; `DAMAGE_RECEIVER_TECHNO_GATES...:130-139`. |
| T6 | Active invulnerability query at virtual `+0x160`, only if `!ignoreDefenses && !originalNegative` | Effect helper, write 0, return 0 | `+0x160 -> 0x0041BF40` tests timer `+0x18C/+0x194`; selector depends on semantic-UNKNOWN `Techno+0x1C4`. |
| T7 | WarpingOut query at virtual `+0x1D4`, only if `!ignoreDefenses` | Write 0, return 0 | `+0x1D4 -> 0x0070C5B0` reads `Techno+0x270`; negative damage is not exempt. |
| T8 | If `Type+0x6B1 DamageReducesReadiness`, mutate signed current ammo/readiness | No direct damage write | Runs regardless of sign, `ignoreDefenses`, warhead, or later immunity. Formula and timer are below. |
| T9 | If bunker/link `Techno+0x2E4` is nonnull and defenses are active, execute the target-kind truth table | Depends on row | This cannot be represented by one generic `bunker_blocked` boolean. |
| T10 | Null warhead after bunker routing | Delegate directly to Object | Radiation/Psychic/Poison/alliance/Psychedelic checks are skipped. |
| T11 | Radiation and target `ImmuneToRadiation`; then PsychicDamage and target `ImmuneToPsionicWeapons`; then Poison and target `ImmuneToPoison` | Each writes 0 and returns 0 | Exact order is load-bearing. |
| T12 | `!AffectsAllies && attacker != null && attackerOwner.IsAlliedWith(targetOwner)` | Write 0, return 0 | `sourceHouse` is not an operand. |
| T13 | Psychedelic prechecks: `targetOwner.IsAlliedWith(sourceHouse)`, target `ImmuneToPsionics`, or runtime target `WhatAmI()==6` Building | Return 0 **without a write** | Uses a different alliance direction/source than T12. |
| T14 | Accepted Psychedelic | Kernel at distance 0; write result to `*pDamage` and `Techno+0x29C`; update `+0x298` and callbacks; return 1 | Object HP receiver is skipped; kernel result may be positive, zero, or negative. |
| T15 | No early return | Call Object with the same seven arguments | Object owns signed HP/callback transaction. |

The readiness calculation at T8 is:

```text
ratio    = (x87) *pDamage / signed Strength
scaled   = float32(ReadinessReductionMultiplier) * ratio
newAmmo  = ftol((double)currentAmmo - (double)maxAmmo * scaled)
current  = max(newAmmo, 0)       // lower clamp only
```

It has one final `ftol`, no Strength-zero guard, and no upper clamp. Negative damage can increase current ammo above maximum. Zero still invokes `0x006FB080`. A later bunker or immunity return does not roll back the mutation (`DAMAGE_RECEIVER_TECHNO_GATES...:165-206`).

`0x006FB080` returns if current ammo is already at least maximum. Otherwise it chooses `EmptyReload` for exact zero when configured, or `Reload + ReloadIncrement * group^2`, where `group` is 1 if `PipWrap==0` and otherwise signed `currentAmmo / PipWrap`. It stores current frame at `Techno+0x1FC`, duration at `+0x204`, and an indeterminate local-stack dword at `+0x200`. It is a reload-timer scheduler, not an animation/audio helper. The meaning and consumers of `+0x200` remain a byte-parity blocker.

The T9 bunker truth table is:

| Runtime target kind | Warhead | `PenetratesBunker` | Native result |
|---|---|---:|---|
| Building (`Object WhatAmI==6`) | null | n/a | Jump to Object; skip warhead gates. |
| Building | nonnull | true | Write zero and return 0. |
| Building | nonnull | false | Continue to warhead gates. |
| Non-Building | null | n/a | Jump to Object; skip warhead gates. |
| Non-Building | nonnull | true | Continue to warhead gates. |
| Non-Building | nonnull | false | Cell lookup; write zero/return only if looked-up Building equals the live link. |

### 3.3 Object receiver transaction

| Order | Native action and fresh-read rule | `pDamage`, HP, result, or callback consequence |
|---:|---|---|
| O1 | Snapshot signed entry Health. | Retained for arithmetic and first-change-from-full. |
| O2 | Return gates: `Health <= 0`, requested damage `== 0`, or `!ignoreDefenses && Type+0x233 Immune`. | Return 0, `pDamage` unchanged. |
| O3 | If defenses are active, call `0x00489180` with signed damage, actual warhead, armor, and signed distance. | Kernel result replaces `*pDamage`. |
| O4 | Runtime Building and `BuildingType+0x1577 CanC4==false`. | `*pDamage=max(*pDamage,1)` after kernel, so zero or healing can become +1 damage. |
| O5 | Normalized damage is zero. | Return 0. |
| O6 | Normalized damage is negative. | `Health=old-pDamage`, cap to fresh Strength, call virtual `+0x148(7)` iff HP changed, return 0. `pDamage` remains the normalized request even if healing was capped. |
| O7 | Positive transaction begins. | Result starts at 1. |
| O8 | Yellow crossing. | Set result 2 only for the signed `Strength >> 1` downward-crossing predicate. Object does not use `ConditionYellow` here. |
| O9 | Inclusive overkill: `pDamage >= oldHealth`. | Write old Health through `pDamage`; equality is included. |
| O10 | Red crossing in x87 double precision against `Strength * Rules+0x1708`. | Strict old-above and new-below comparisons; result becomes 3 and overwrites 2. |
| O11 | Commit signed `Health = old - pDamage`. | Later callbacks see committed HP. |
| O12 | One-time Cyborg survival: runtime Infantry `WhatAmI==0x0F`, `!ignoreDefenses`, InfantryType Cyborg, instance flag clear. | Optional animation, HP=`max(ftol(Strength*0.25),1)`, set flag, call `+0x558(6,1,0)`, force result 3. This is not Building/Veinhole behavior. |
| O13 | Snapshot post-base HP, then threshold triggers. | Result 2: `0x27` requires attacker, then fresh Alive, `0x2A`, then fresh Alive. Result 3: analogous `0x28`, then `0x2B`. |
| O14 | Recompute first-change-from-full from fresh HP/type/Strength. | If true: `0x26` requires attacker; first `0x29` uses sourceObject null; reread tag/Alive; second `0x29` requires attacker and uses attacker as sourceObject. |
| O15 | Fresh Alive and saved-positive/fresh-HP guard. | A callback death from a positive post-base snapshot returns forced result 5 rather than entering credit routing. |
| O16 | Fresh exact `Health == 0` death gate. | Trigger healing can rescue zero; a callback-created negative HP does not equal zero. |
| O17 | Select kill-credit callback. | `sourceHouse==null`, or same as nonnull attacker owner -> virtual `+0xE0(attacker)`; otherwise `+0xE4(sourceHouse)`. |
| O18 | After credit callback, set result 4 and call virtual `+0xDC(1)`, then reread Alive. | Credit precedes destruction. Object route can award XP; House route never calls `AddExperience`. |
| O19 | Surviving non-result-4 tail. | Trigger `0x06`, reread Alive; trigger `0x2C`, reread Alive; if result nonzero and selected, call `+0x124(2)` last. |

The complete machine-order ledger and exact trigger arguments are in `DAMAGE_RECEIVER_OBJECT_CALLBACKS...:139-184`. `0x006E53A0` runs the current tag synchronously and reaches action execution before returning (`...:186-202`). Therefore every explicit tag, Alive, type, Strength, and Health reread is part of the behavior contract.

Working numeric result labels are: 0 no receiver state, 1 damaged/accepted Psychedelic marker, 2 yellow crossing, 3 red crossing or Cyborg rescue, 4 native death route, and 5 callback/liveness stop. Numeric values and branches are authoritative; friendly enum names are not required for implementation.

### 3.4 Negative kernel correction

`0x00489180` receives armor as stack argument 1 and signed distance as stack argument 2. Its negative branch compares **signed distance** with 8:

```text
damage < 0 and distance < 8  -> return damage
damage < 0 and distance >= 8 -> return 0
```

Positive Verses lookup independently uses stack argument 1 as armor. The old “armor indices 8-10 block healing” interpretation is false (`DAMAGE_RECEIVER_TECHNO_GATES...:279-306`; `DAMAGE_RECEIVER_OBJECT_CALLBACKS...:85-97`).

### 3.5 Immediate Techno postlude: verified boundary, incomplete internals

After Object returns, Techno performs threat feedback even for result 0; branches on result 4/5; writes last-damage timing/distance fields for nonzero results; conditionally runs the trainable/readiness-disabled response; writes `WasAttacked` for a hostile attacker; calls virtual `+0xFC`; maintains health particles; then skips retaliation for the original-negative snapshot or enters `ShouldRetaliate`/retaliation/scatter (`DAMAGE_RECEIVER_TECHNO_GATES...:308-347`).

The shared order is verified. The state-4 delayed-death internals, full retaliation/scatter mechanism, semantic role of virtual `+0xFC`, and exact name of `Techno+0x1C4` are not. This bounded PARTIAL must not be upgraded to COMPLETE.

## 4. Native field, default, parser, writer, and Rust matrix

Legend: **C** = closed for the bounded mechanism; **P/G1** = partial and blocks G1; **MISSING** describes current Rust and does not by itself weaken native evidence.

| Native value | Default / parser / stock result | Runtime writers or consumer | Closure | Current Rust state |
|---|---|---|---|---|
| Object Health `+0x6C`, type Strength `+0xA0`, `pDamage` | Signed int32 in receiver | Object transaction and synchronous callbacks | **C** | `Health.current/max` are `u16`; live events are `u16`. |
| ObjectType `+0x233 Immune` | false; `Immune=` current-value fallback; stock mixed | Object entry gate | **C** | Standard `ObjectType` field/parser missing. |
| ObjectType `+0x232 Insignificant` | false; distinct `Insignificant=` field | Not the Object damage gate | **C** | Standard field missing; terrain has a separate default-true field. |
| BuildingType `+0x1577 CanC4` | true for Buildings; four stock `no` | Object post-kernel floor | **C** | Parsed with category default. |
| InfantryType Cyborg / instance one-shot flag | false; no active stock `Cyborg=yes` | One-time 25% rescue | **C** | Type bool exists for a different documented spark consumer; receiver branch missing. |
| Veteran abilities `TechnoType+0x29C..0x2AD` | 18 zero bytes; present key replaces all 18 | Rank-selected `STRONGER` index 1 and `FIREPOWER` index 2 | **C** | Only FEARLESS is extracted. |
| Elite abilities `TechnoType+0x2AE..0x2BF` | Same; not stale `+0xAB8` | Same rank selection | **C** | Only FEARLESS is extracted. |
| `VeteranArmor Rules+0x688`, `VeteranCombat Rules+0x670` | stock 1.5 and 1.1 | Separate receiver divide / attacker multiply | **C** | Missing from rules authority; shadow accepts preassembled values only. |
| TypeImmune `TechnoType+0xC8C` | false; stock `DLPH` and `YURI` true | Techno prefix | **C** | Schema missing; unsourced shadow bool. |
| Psionic immunities `+0xD35/+0xD36` | Building constructors true; Aircraft/Infantry/Unit false; INI overrides | Psychedelic / PsychicDamage gates | **C** | Missing; a uniform default would be wrong. |
| Radiation `+0xD37`, Poison `+0xD3B` immunities | false; INI overrides | Ordered warhead gates | **C** | Radiation exists live; Poison missing. |
| Readiness flag `+0x6B1`, multiplier `+0x6B4` | false / float32 0.0; stock-disabled | T8 formula | **C** for reads/math | Missing. |
| Type InitialAmmo `+0x680`, max Ammo `+0x684` | both -1; 13 stock finite Ammo rows | Init of instance current ammo | **C** for construction/read | Rust parses Ammo but only provisions Aircraft. |
| Techno current ammo `+0x2FC` | signed int32; InitialAmmo unless -1, else Ammo | readiness, firing, reload, class-specific paths | **P/G1** | Aircraft-only optional state; exhaustive class meanings/writers/save-load-reset open. |
| Reload timer `+0x1FC/+0x200/+0x204` | runtime | `0x006FB080` | **P/G1** | No equivalent; `+0x200` consumer/meaning open. |
| Techno armor instance `+0x158` double | ctor 1.0; stock armor crate 1.5 | receiver grouped divisor; raw save/load | **C** for bounded arithmetic | Missing. |
| Techno firepower instance `+0x160` double | ctor 1.0; stock firepower crate 2.0 | attacker fire build | **P/G1** | Missing; writer/save/load/reset/removal set not exhaustively closed. |
| Techno veterancy `+0x150` float32 | ctor 0; setters write 0/1/2 including false-demotion; raw save/load | selects ability tier | **C** for bounded rank input | `u16` 0/100/200 model; exact writer equivalence unproven. |
| Difficulty FirePower/Armor | three 0x50-byte slots; defaults 1; stock Armor 1.2/1.0/0.8 | `HouseClass::SetDifficulty` | **C** for formula | No combat difficulty schema/assembly. |
| Country global Firepower `HouseType+0xC8`, Armor `+0xE0` | ctor 1.0; stock neutral | Applied only in multiplayer SetDifficulty | **C** | Missing. |
| House Firepower `+0x188`, Armor `+0x1A0` | neutral then SetDifficulty | Fire build / receiver | **P/G1** for full lifecycle | `HouseState` has no combat multipliers. Normal serialization/reapply was not reopened. |
| Category armor floats `HouseType+0x100..+0x110` | all 1.0; stock neutral | live `0x0050BD30` lookup | **C** after fresh conflict check | Missing. |
| Warhead `PenetratesBunker`, `PsychicDamage`, `AffectsAllies` | false, false, **true** respectively | Techno gates | **C** native | Missing from `WarheadType`. |
| Warhead Radiation, Poison, Psychedelic | false; INI overrides | Techno gates | **C** native | Present, but current comments contain stale offsets. |
| `ConditionYellow +0x1700`, `ConditionRed +0x1708` | doubles 0.5/0.25; stock same | particles/presentation and Object red crossing | **C** native | Stored f32 plus x1000; exact equivalence unproven. |
| `DamageFireTypes` | General key; stock FIRE01/FIRE02/FIRE03 | Building presentation | **C** for source/list | Present; concrete RNG/list consumption outside this slice. |
| `BuildingDamageSound Rules+0x714` | AudioVisual key; stock `BuildingDamaged` | Building wrapper presentation | **P/G1** | Missing; constructor fallback remains open. |
| Warhead `Sparky` | false; stock explicit rows also false | concrete Building branch | **C** native | Missing. |
| `ParentCountry` combat inheritance | current country fields are read directly in checked paths | any unseen inheritance/copy path | **P/G1** | No equivalent combat chain. |
| Concrete wrapper-only receiver sources | child 1C lacked the wrapper report at closure | Foot/Building/Terrain/etc. policies | **P/G1** | Cannot declare a complete schema/cutover. |

Fresh category lookup reconciliation:

| Type object's class-kind code | HouseType float | INI key |
|---:|---:|---|
| `0x10` | `+0x100` | `ArmorInfantryMult` |
| `0x28` | `+0x104` | `ArmorUnitsMult` |
| `3` | `+0x108` | `ArmorAircraftMult` |
| `7`, Building type `+0xE08 != 5` | `+0x10C` | `ArmorBuildingsMult` |
| `7`, Building type `+0xE08 == 5` | `+0x110` | `ArmorDefensesMult` |

At `0x00701945`, the caller obtains the **target type pointer** through target virtual `+0x84`; at `0x00701952` it calls `0x0050BD30`. The helper switches on the type object's class-kind, not the runtime target object's WhatAmI. Parser xrefs independently bind `ArmorInfantryMult -> +0x100` at `0x00511AD7`, Units `-> +0x104` at `0x00511AF6`, Aircraft `-> +0x108` at `0x00511B15`, Buildings `-> +0x10C` at `0x00511B34`, and Defenses `-> +0x110` at `0x00511B53`. This distinction reconciles runtime Building `WhatAmI==6` in the receiver with BuildingType class-kind 7 in the category helper.

Fresh ability-array reconciliation: assembly at `0x007154A3` uses `LEA EDI,[EBP+0x29C]` for Veteran; `0x007154E8` uses `LEA EDI,[EBP+0x2AE]` for Elite. Parser `0x00477640` replaces all 18 destination bytes when the key is present and preserves/copies the prior 18 bytes when absent.

## 5. Contradiction and stale-wording ledger

The quoted text below is superseded exactly as stated; the older files were intentionally not patched in this synthesis.

| Source and exact stale wording | Classification | Replacement authority |
|---|---|---|
| Task 1 charter wording: “corrected gate/design identify armor index” for the negative kernel predicate | WRONG operand | The signed comparison is `distanceLeptons < 8`, not armor. Child 1A plus cold assembly `0x004891AF..0x004891C3`. |
| `RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md:337-344`: `return (armorType > 7) ? 0 : damage;` and “Special armors ... block healing” | WRONG | Replace `armorType` with signed distance; armor is the other stack argument. Lines 477-485 are stale for the same reason. |
| `src/sim/combat/damage/kernel.rs:54-57`: “armor index >= 8 ... cannot heal” | IMPLEMENTATION DRIFT | Use signed `distance_leptons < 8`; test distance -1, 0, 7, 8, 9 across multiple armor indices. |
| `GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md:47-51`: `3 Infantry +0x108`, `0x10 Aircraft +0x100`, `0x28 Building +0x104`, and runtime/flying-unit labels | WRONG labels/mapping | Use the fresh type-kind table in section 4. Do not call these codes runtime Object WhatAmI values. |
| `DAMAGE_MATH_GHIDRA_REPORT.md:233-240` repeats the old kind labels; line 244 says `damage = ftol((float)damage * countryArmorMult)` | WRONG mapping and operation | Receiver **divides** by House/category/instance product, then `ftol`. |
| `TECHNOTYPECLASS_BASE_GHIDRA_REPORT.md:451,641-643`: Elite at `[0xAB8 .. 0xAC9]` | WRONG base | Elite abilities are 18 bytes at `+0x2AE..+0x2BF`, fresh assembly-verified. |
| `RECEIVE_DAMAGE_GHIDRA_REPORT.md:63-65`: TypeImmune “damage is zeroed and returns 0” | WRONG write semantics | It returns 0 without writing again; transformed `pDamage` remains visible. |
| `RECEIVE_DAMAGE_GHIDRA_REPORT.md:126,513` and `RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md:203,756`: `+0x233` is “Insignificant” | WRONG identity | `ObjectType+0x233` is `Immune`; `+0x232` is `Insignificant`. |
| `RECEIVE_DAMAGE_GHIDRA_REPORT.md:87,597`: `0x006FB080` is an “Ammo depletion animation trigger” | WRONG role | It schedules/restarts reload timer fields; no direct animation/audio call is present. `+0x200` remains unresolved. |
| `RECEIVE_DAMAGE_GHIDRA_REPORT.md:414`: `PenetratesBunker` “Bypasses ForceShield/bunker protection” | WRONG generalization | Use the target-kind truth table. Linked Building + `PenetratesBunker=true` nullifies the hit. |
| `GATE_DAMAGE_COUNTRY_ARMOR_ORDER_RESOLUTION_GHIDRA_REPORT.md:101`: `+0x160` WarpingOut, `+0x1D4` ForceShield/invulnerability, generic bunker, Psychedelic “zero HP” | MULTIPLE WRONG claims | `+0x160` is active invulnerability query; `+0x1D4` reads WarpingOut; bunker is branch-sensitive; accepted Psychedelic stores kernel state and skips Object HP. |
| `RECEIVE_DAMAGE_PIPELINE_VERIFICATION_REPORT.md:272-281` and `DAMAGE_MATH_GHIDRA_REPORT.md:448-460`: WhatAmI `0x0F` Building/Veinhole survival | WRONG class | `InfantryClass::WhatAmI` returns `0x0F`; this is one-time Cyborg Infantry survival. |
| `DAMAGE_MATH_GHIDRA_REPORT.md:480`: event `0x29` is “Any damage dealt” | WRONG predicate | Both `0x29` calls are inside the first-change-from-full gate, separated by fresh tag/Alive reads and with different sourceObject arguments. |
| `VETERANCY_SYSTEM_GHIDRA_REPORT.md:227-238`: House `+0x2BF` propagates `[SpecialFlags] InitialVeteran` | SELF-CORRECTED STALE SECTION | The same document corrects this at 1682-1689: `+0x2BF` is the spy-infiltrated War Factory future-infantry bonus; global InitialVeteran is distinct. |
| `DAMAGE_RECEIVER_RULE_HOUSE_ASSEMBLY...:104-108` calls category codes `WhatAmI` | IMPRECISE, values otherwise correct | Say “target **type object's class-kind**”; offsets/key mapping remain correct after the fresh check. |

## 6. Explicit G1 authority table

G1 passes only if every load-bearing Task 1 acceptance row is closed. Frequency, stock neutrality, or an implementable-looking partial mechanism cannot downgrade an authority gap.

| Task / acceptance row | Result | Evidence or blocker |
|---|---|---|
| 1A function identity, active reachability, seven-slot pass-through | **PARTIAL** | Address/vtable/callers and slot behavior are verified, but `arg6_unknown` lacks semantic identity required by the task's exact-argument bar. |
| 1A original-sign snapshot and per-exit `pDamage` semantics | **PASS** | Exact writes/no-writes and state returns decided. |
| 1A House/category/instance armor grouping, rank divide, conversion points, incoming zero | **PASS** | x87/assembly plus fresh category reconciliation. |
| 1A TypeImmune predicate and write behavior | **PASS** | Exact type/owner comparisons; no zero write. |
| 1A invulnerability and WarpingOut roles/predicates | **PARTIAL** | Slot roles and receiver behavior pass; `Techno+0x1C4` effect-selector semantics remain UNKNOWN. |
| 1A readiness formula, widths, order, and lower-only clamp | **PASS** | Formula and timing relative to later gates are closed. |
| 1A readiness helper/timer byte behavior | **FAIL** | `Techno+0x200` meaning and all consumers unresolved; native writes indeterminate stack data. |
| 1A bunker/link truth table | **PASS** | All Building/non-Building, null-warhead, and PenetratesBunker branches decided. |
| 1A Radiation/Psychic/Poison/AffectsAllies/Psychedelic | **PASS** | Exact order, identities, alliance operands, write semantics, and accepted state path decided. |
| 1A negative kernel operand | **PASS** | Signed distance threshold verified and older armor interpretation rejected. |
| 1A Object delegation | **PASS** | Same seven values and order. |
| 1A common Techno postlude helpers | **FAIL** | State-4 internals, full retaliation/scatter, and virtual `+0xFC` semantics excluded/unresolved. |
| 1A mutable armor/rank writer closure | **PASS** for bounded `+0x158/+0x150` | Constructor, active writers, demotion, and raw save/load bounded. |
| 1A readiness/current-ammo full provenance | **FAIL** | Exhaustive `+0x2FC` class-specific writer/save/load/reset/removal chain absent. |
| 1B signed HP/pDamage widths, entry gates, kernel handoff | **PASS** | Raw 32-bit operations and exact gates closed. |
| 1B CanC4 floor, healing/cap, inclusive overkill, result 0-5 | **PASS** | Every write/branch and threshold comparison closed. |
| 1B one-time Cyborg branch | **PASS** | Runtime class, type flag, instance flag, writes, callback arguments closed. |
| 1B trigger codes/predicates/order/arguments and refresh checkpoints | **PASS** | Complete machine-order ledger including distinct 0x29 calls. |
| 1B synchronous mutation/re-entry visibility | **PASS** | Dispatcher/action runner and concrete mutating actions checked. |
| 1B death gate, object-vs-house credit, XP/no-XP split, destruction order | **PASS** for bounded receiver boundary | Tail counters outside the receiver contract remain explicitly out of scope. |
| 1B survivor events and selected refresh | **PASS** | Exact order and fresh Alive rereads closed. |
| 1C Veteran/Elite arrays, bases, indices, replacement semantics, stock rows | **PASS** | Fresh array checks resolve the only load-bearing doc conflict. |
| 1C Object/Techno immunity fields and class-specific defaults | **PASS** | Constructor/parser/stock matrix closed. |
| 1C readiness type fields and initial/max/current construction | **PASS** for constructors/reads | Runtime writer lifecycle is a separate failed row. |
| 1C difficulty/country/House/category assembly formula and SP/MP split | **PASS** for formula | `SetDifficulty` and fresh category helper mapping closed. |
| 1C House combat fields full initialization/reapply/save-load lifecycle | **PARTIAL/FAIL** | Normal House serialization and exhaustive reapplication/reset were not reopened. |
| 1C Techno armor instance and veterancy writers | **PASS** for bounded rows | `+0x158/+0x150` closed. |
| 1C Techno firepower instance full writer/save-load/reset/removal | **FAIL** | `+0x160` closure absent. |
| 1C Techno current ammo full writer/save-load/reset/removal | **FAIL** | `+0x2FC` closure absent. |
| 1C global thresholds, DamageFireTypes, Warhead defaults/gates | **PASS** for native source/default | Rust representation remains drift/missing. |
| 1C BuildingDamageSound source/default | **FAIL** | Key/stock handle known; constructor fallback unknown. |
| 1C ParentCountry combat inheritance | **FAIL** | No copy was found in checked paths, but outside-path inheritance was not exhaustively ruled out. |
| 1C exhaustive concrete-wrapper field/source matrix | **FAIL** | Required parallel wrapper evidence was absent at child closure. |
| 1S contradiction reconciliation | **PASS** | Category mapping and Elite base conflicts resolved; stale wording explicitly superseded. |
| 1S unified implementation handoff and Rust scan | **PASS** | Sections 7-8 define the bounded handoff without claiming cutover readiness. |
| 1S no authority-critical UNKNOWN remains | **FAIL** | Multiple failed rows above remain load-bearing. |

**G1 VERDICT: FAILED.** Tasks that require a certified receiver authority surface (including final implementation contracts/cutover work) must not treat this report as a pass. Schema-only shadow work can proceed only if it keeps unresolved semantics explicit and does not become live authority.

## 7. Focused current-Rust scan

| Surface | Current behavior | Verified native disparity |
|---|---|---|
| `src/sim/combat/damage/mod.rs:20-21` | Explicit additive shadow; not wired live | Correctly non-authoritative, but comments/inputs already contain stale identities. |
| `damage/receive.rs:49-66` | Prefix only for `dmg > 0`; zero-divisor guards | Native condition is `!ignoreDefenses && !originalNegative`, includes zero, and has no divisor guards. The function lacks `ignoreDefenses`. |
| `damage/gates.rs:14-56` | One unconditional sequence over precomputed booleans | Native predicates depend differently on sign, ignore, live link/type, two alliance relations, warhead nullability, and readiness side effects. |
| `damage/mod.rs:98-101` | Names `+0x160` WarpingOut and `+0x1D4` ForceShield | Reversed: virtual `+0x160` is active invulnerability; `+0x1D4` reads WarpingOut. |
| `damage/mod.rs:102-119` | Collapsed bunker bool; one `is_allied`; Psychedelic marker | Loses bunker truth table, attacker-owner vs sourceHouse distinction, and accepted Psychedelic kernel/state writes. |
| `damage/receive.rs:68-75` | `DamageGate::Nullified` collapses all early exits | Cannot represent unchanged vs transformed vs zeroed `pDamage`, state, or readiness timing. |
| `damage/kernel.rs:54-57,121-128` | Blocks negative by armor index >=8 and tests it | Wrong operand; signed distance boundary is native. |
| `damage/receive.rs:90-111` | Returns a pure HP delta/classification | Omits Object's signed in/out writeback, healing callback, trigger transaction, result 4/5, and death/credit order. |
| `damage/mod.rs:142-150` | Result enum has only five named variants 0-4 style | Native has numeric result 0 through 5 with forced liveness exits. |
| `src/sim/combat/mod.rs:1195,1851-1885` | Damage events contain `u16`; live apply checks coarse invulnerability then `saturating_sub`, fear, dead list, last attacker | No signed request/healing, mutable writeback, native gates, readiness, callbacks, result, or ordering. Warhead ID is ignored at apply. |
| `src/sim/combat/mod.rs:1896-1909` | Death handling is deferred after all damage events | Native credit/destruction happens inside each receiver after synchronous callbacks and fresh reads. |
| `src/sim/components.rs:78-88` | Health current/max are `u16` | Native receiver uses signed int32 HP/Strength operations and permits callback-created negative states to differ from exact zero. |
| `src/sim/game_entity.rs:288-289,348-371` | Last attacker, invulnerability, and aircraft-only ammo exist | These are not the native receiver transaction; general signed Techno ammo/readiness, link/status/postlude fields are absent. |
| `src/rules/object_type.rs:401-435,1034,1049-1050` | Ammo parsed; only FEARLESS extracted from abilities | Full 18-byte arrays, STRONGER/FIREPOWER, InitialAmmo, and general current ammo are missing. |
| `src/rules/warhead_type.rs:63-104,162-192` | Several effects parsed | `AffectsAllies`, `PenetratesBunker`, `PsychicDamage`, and `Sparky` absent; comments use stale offsets. |
| `src/sim/house_state.rs:19-55` | Identity/economy core only | No House Firepower/Armor or difficulty/country/category combat assembly. |
| `src/sim/trigger_runtime.rs:1-14,177-211` | Intentionally narrow global/time/variable/TechType trigger runtime | No per-object AttachedTag receiver events `0x06/0x26..0x2C`, synchronous receiver action boundary, or tag mutation API. |
| `src/rules/ruleset.rs:283-313,1079-1084,1171-1174` | Condition ratios stored as f32 and x1000 | Native thresholds are doubles; byte-identical/full-input equivalence is unproven. |

The existing damage-unit tests are useful regression ratchets only. They are not gamemd parity evidence, and several currently lock known-wrong behavior (armor-gated healing, zero-damage prefix, reversed virtual labels, coarse nullification, and zero-HP Psychedelic handling).

## 8. Rust-facing contract and acceptance tests

Implementation must remain Rust-native in ownership while preserving this native transaction. A viable shape is a sim-owned receiver service with explicit entity/lifecycle/trigger callbacks; it must not move gameplay ownership into render/audio or recreate C++ class plumbing.

| Required Rust contract | Required behavior | Candidate owner | Minimum acceptance evidence/test |
|---|---|---|---|
| Signed damage request | `i32` in/out damage, distance, warhead, attacker, ignore, arg6 pass-through, sourceHouse; preserve original sign | `sim/combat` receiver API | `techno_receiver_damage_writeback_matrix`; oracle fixtures for unchanged/transformed/zero/Psychedelic/overkill. |
| Ordered Techno prefix | Two armor conversions, no invented guards, zero -> 1, exact TypeImmune/status predicates | receiver transaction | `techno_receiver_prefix_sign_zero_and_status_order`. |
| Readiness state | General signed current/max ammo, exact formula, timer side effect before later gates | entity readiness/ammo component plus timer primitive | `techno_receiver_readiness_before_immunity`; cutover blocked until `+0x200/+0x2FC` closure. |
| Live bunker operands | Preserve link pointer/ID, runtime kind, target cell lookup, actual warhead/null and flag | docking/link plus receiver | `techno_receiver_bunker_truth_table`. |
| Separate alliance checks | AffectsAllies uses attacker owner -> target owner; Psychedelic uses target owner -> sourceHouse | House/alliance service | `techno_receiver_alliance_operand_matrix`. |
| Psychedelic state | Rejection no-write; accepted kernel distance 0, dual state writes, callbacks, result 1, no Object HP | receiver + mission/lifecycle authority | `techno_receiver_psychedelic_state_transition`. |
| Correct negative kernel | Signed distance threshold 8 independent of armor | one shared damage kernel | `damage_kernel_negative_distance_boundary`; oracle-derived values at -1/0/7/8/9. |
| Signed Object HP transaction | Immune, kernel writeback, CanC4 floor, healing cap/callback, inclusive overkill | Object-level receiver over EntityStore | `object_receive_damage_signed_request_matrix`. |
| Native result and crossings | Result 0..5; signed half-Strength yellow; strict double red | receiver result enum | `object_receive_damage_condition_crossing_boundaries`, including odd Strength and equality. |
| Synchronous tag callbacks | Exact events, arguments, fresh rereads, nested mutation allowed | trigger runtime + lifecycle authority | `object_receive_damage_callback_order_and_mutation`. |
| Death/credit transaction | Positive-snapshot guard, exact zero test, object-vs-house callback, XP split, credit before destruction | receiver + House/veterancy/lifecycle | `object_receive_damage_death_credit_route`; `object_receive_damage_record_kill_xp_boundary`. |
| Survivor tail | `0x06`, Alive, `0x2C`, Alive, selected refresh | receiver tail | `object_receive_damage_survivor_tail_refresh_order`. |
| Full type/rule schema | 18-byte replace-on-present arrays; class-specific immunities; all warhead fields/defaults | `rules/` | `damage_receiver_type_defaults_and_md_overlay`. |
| House modifier assembly | Difficulty slots; SP/MP country rule; live type-category lookup; per-instance/rank stages | House state plus rules | `damage_house_modifier_assembly_sp_mp_categories`. |
| Runtime writer round-trip | crate armor/firepower, promotion/demotion, ammo firing/reload/readiness, save/load/reset/removal | entity serialization and systems | `damage_receiver_runtime_modifier_roundtrip`; blocked until provenance rows close. |
| Presentation handoff | Condition doubles, BuildingDamageSound, DamageFireTypes, Sparky, deterministic ordering | sim event -> audio/render consumers | `building_damage_presentation_event_order`; blocked on wrapper/default closure. |

Every parity acceptance must name a gamemd/retail-derived executable oracle or an exhaustive proof. Rust-vs-Rust snapshots and hand-computed constants can guard regressions but cannot certify parity.

## 9. Do-not-do constraints

- Do not wire the current shadow receiver into live combat while G1 is failed.
- Do not collapse the transaction to a final HP delta or queue trigger actions for later.
- Do not model `pDamage` as unsigned, input-only, or always zero on an early return.
- Do not treat incoming zero as an unconditional no-op; Techno can turn it into 1 before later gates.
- Do not invent zero-divisor or Strength-zero guards and call them native parity.
- Do not use armor to gate negative kernel damage; use signed distance.
- Do not reuse one alliance boolean for AffectsAllies and Psychedelic.
- Do not replace the bunker truth table with “PenetratesBunker bypasses bunker protection.”
- Do not label virtual `+0x160` WarpingOut or virtual `+0x1D4` ForceShield.
- Do not confuse virtual slot `+0x160` with data field `Techno+0x160` firepower.
- Do not confuse runtime Object WhatAmI with target **type** class-kind in `0x0050BD30`.
- Do not parse only FEARLESS or infer FIREPOWER from STRONGER; `TELE` is a stock counterexample.
- Do not use one psionic constructor default for every Techno class.
- Do not invent `SelfHealC4`; SELF_HEAL ability, ability C4, Infantry C4, and Building CanC4 are distinct.
- Do not assign a deterministic value to timer `+0x200` until its consumers prove a byte-equivalent Rust representation.
- Do not call f32/x1000 threshold handling exact without exhaustive equivalence proof against native double behavior.
- Do not use GitHub/YRpp/Ghidra labels as authority over active binary bodies and verified retail data.

## 10. Remaining uncertainty and smallest next investigations

### Handoff A - runtime mutable-state closure

1. Follow every consumer of reload-timer `Techno+0x200`, decide the role of the indeterminate write, and close `0x006FB080` scheduling semantics.
2. Exhaustively map class-specific `Techno+0x2FC` initialization, firing/reload/readiness writers, save/load, reset, and removal.
3. Exhaustively close `Techno+0x160` firepower crate writer, serialization, reset/removal, plus House `+0x188/+0x1A0` reapplication/save-load lifecycle.

### Handoff B - receiver ABI and common-tail closure

1. Resolve the semantic identity and all active callers of argument slot 6.
2. Resolve `Techno+0x1C4` and virtual `+0xFC` enough to name their state contract without relying on local labels.
3. Complete result-4 delayed-death, `ShouldRetaliate`, retaliation, flee/scatter, threat, and particle helper behavior required by the original Task 1A postlude bar.

### Handoff C - wrapper/default/inheritance closure

1. Produce the exhaustive concrete receiver-wrapper field/source/order matrix (Foot, Building, Terrain, and all active derivatives in scope).
2. Close `BuildingDamageSound` constructor fallback and concrete Building presentation/RNG consumers.
3. Exhaustively decide whether `ParentCountry=` copies/inherits combat fields outside checked `ReadINI`/`SetDifficulty` paths.

Until all G1-failed rows are closed and reconciled, the strongest honest combined status remains:

> **Synthesis COMPLETE; native authority PARTIAL; G1 FAILED; live receiver cutover blocked.**
