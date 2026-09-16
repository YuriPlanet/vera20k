# Refinery Storage Flow — Ghidra Research Report

**Addresses (primary functions):**
- `0x73D630` — `UnitClass::Mission_Deploy_Building` (the Allied/Soviet harvester unload state machine, state 3 = drain loop)
- `0x522D50` — `BuildingClass::DepositOreFromStorage` (the Yuri Slave Miner drain function)
- `0x4F9610` — `HouseClass::Add_Tiberium_Credits`
- `0x6C9690` / `0x6C96B0` / `0x6C9650` / `0x6C9820` — `StorageClass::AddAmount/RemoveAmount/GetTotalAmount/FindFirstNonEmptySlot`
- `0x4509D0` phase F — `BuildingClass::UpdateAnimation` (tier visual)

**Confidence:** HIGH for the drain-flow chain (decompiled directly), MEDIUM for some struct-field semantics (offset purpose inferred, not all reader/writer pairs traced).

**Active in YR:** Yes (standard Allied/Soviet path), Yes (Yuri Slave Miner path); NO (the Add_Tiberium_To_Storage branch — TS-legacy Weeder).

---

## 1. Overview

Yuri's Revenge has **two structurally distinct ore-deposit paths**:

1. **Allied/Soviet (standard harvester)** — the harvester carries its own `StorageClass` and drains it inline during `Mission_Deploy_Building` state 3 → `HouseClass::Add_Tiberium_Credits`. The refinery's `StorageClass` is **not touched** by this path. The refinery's `Storage=` value is consumed only by `UpdateAnimation` phase F to drive the tier-display visual.
2. **Yuri (Slave Miner)** — slave infantry harvest into their **own** `StorageClass` at `+0x33C` (`FUN_00522E70` writes `LEA EDI,[ESI+0x33C]` at `0x00522F0A` with `ESI` = the slave, then calls `StorageClass::AddAmount`). The slave's own storage is then drained to credits via `0x00522D50` (`DepositOreFromStorage`), which is called with `ECX` = the slave (`LEA EBP,[ECX+0x33C]` at `0x00522D55`); the Slave Miner building is only the stack argument, used for its `+0x21C` owner and the `vtable+0x468` smoke callback. It is called from `SlaveManagerClass::AI_Update` state 4 when a slave reaches the dock cell. (Corrected 2026-09-06 against the binary; the building's `StorageClass` is never written by this path.)

Both paths use identical bonus math — `bonus = facilities × PurifierBonus × amount` — and award credits via `HouseClass::Add_Tiberium_Credits`. The Ore Purifier bonus and AIVirtualPurifiers difficulty bonus apply to both.

A third path exists (`HouseClass::Add_Tiberium_To_Storage`, gated by `UnitTypeClass+0xE0F` = `Weeder=yes`), but it is **TS-legacy Weeder harvester code, never reached in a standard YR skirmish**.

The refinery storage-tier visual indicator is a separate concern from credits — it reads `StorageClass::GetTotalAmount` on the refinery itself, computes `tier = floor(4 × stored / Storage)`, and clears+creates one of slots 3/4/5/6 (`ActiveAnim`/`Two`/`Three`/`Four` = `GAREFNL1..L4`).

## 2. Class Layout / Key Offsets

### StorageClass (4 floats, 16 bytes total)

| Offset | Type | Purpose |
|--------|------|---------|
| `+0x00` | float | Slot 0 (TS Riparius / RA2 Ore) |
| `+0x04` | float | Slot 1 (TS Vinifera / RA2 Gems) |
| `+0x08` | float | Slot 2 (TS Cruentus — unused in YR) |
| `+0x0C` | float | Slot 3 (TS Aboreus — unused in YR) |

Used both standalone (per harvester unit, per refinery building) and indirectly via `HouseClass::Add_Tiberium_To_Storage` (which operates on whatever StorageClass instance is implied by the static caller; the function signature itself doesn't take a HouseClass pointer — see Open Questions §7.4).

### BuildingClass (refinery-relevant fields)

| Offset | Type | Purpose | Confidence |
|--------|------|---------|------------|
| `+0x87` (= byte `0x21C`) | `HouseClass*` | Owner pointer | HIGH (used in DepositOreFromStorage) |
| `+0x584` | anim handle | Slot-10 SpecialAnim handle (per-bale flash) | HIGH (decompile-verified) |
| `+0x6F0` | int | Cached storage tier (0/1/2/3+) for the visual | HIGH (verified previous session) |

### BuildingTypeClass (refinery-relevant fields)

| Offset | Type | Purpose | Confidence |
|--------|------|---------|------------|
| `+0x800` | int | `Storage=` capacity | HIGH (used as denominator in tier formula and as cap in Harvest_Ore_Tick) |
| `+0x16BB` | bool (byte) | `Refinery=yes` flag (gates phase F) | HIGH (verified previous session) |
| `+0x16CC` | bool (byte) | `OrePurifier=` flag (bound in `BuildingTypeClass::ReadINI`: string xref `0x004604ED`, write `0x004604FA`); each owned OrePurifier building increments HouseClass+0x538C (corrected 2026-09-06: earlier `+0x16D8` attribution was wrong) | HIGH (binary) |

### UnitClass (harvester-relevant fields)

| Offset | Type | Purpose | Confidence |
|--------|------|---------|------------|
| `+0xF8` (= `param_1[0x3E]`) | int | **Per-bale dump counter.** Reset to 0 on each successful drain. Gate condition: `HarvesterDumpRate × 900 ≤ counter`. | HIGH (used in Mission_Deploy_Building gate) |
| `+0x118` (= `param_1[0x46]`) | int | Step counter — increment site not pinned down this pass | MEDIUM |
| `+0x6C4` (= `param_1[0x1B1]`) | `UnitTypeClass*` | Harvester's type pointer | HIGH |
| `+0x6AC` | byte | (Used in Mission_Deploy_Building early-out) | MEDIUM |

### UnitTypeClass

| Offset | Type | INI key | Purpose |
|--------|------|---------|---------|
| `+0xE0F` | bool (byte) | `Weeder=yes` | Routes deposit to `Add_Tiberium_To_Storage` instead of `Add_Tiberium_Credits` |
| `+0xE0E` | bool (byte) | likely `Tiberium=yes` (TS-era vein harvester) | Same alternate-path gate as a sibling flag |
| `+0xE13` | byte | Used in the "alternative deploy" pre-check (state 0 path) | LOW (not traced) |
| `+0x800` | int | `Storage=` (the unit's cargo capacity in tib units) | HIGH (verified — read by Harvest_Ore_Tick) |

### HouseClass

| Offset | Type | Purpose | Confidence |
|--------|------|---------|------------|
| `+0x184` | int | **AI player difficulty index** (0/1/2 for Hard/Medium/Easy, used to index AIVirtualPurifiers) | HIGH — resolved 2026-05-19 via `HouseClass::SetDifficulty @ 0x004F6EC0` decompile and INI comment `AIVirtualPurifiers=4,2,0 ; h,m,e` (rulesmd.ini:89). See [ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md). |
| `+0x1EC` | bool (byte) | `IsHuman` flag (non-zero = human player) | HIGH (gates the AIVirtualPurifiers add) |
| `+0x30C` | int | Credits — likely the cached/display value | MEDIUM (written by Add_Tiberium_Credits; relationship to +0x54E8 unconfirmed — see Open Q §7.3) |
| `+0x538C` | int | **OrePurifier building count** — used directly as the bonus multiplier in the PurifierBonus formula. Incremented at `OnConstructionComplete` (`0x0044637C`) and `ChangeOwner` new-owner side (`0x004491EB`), decremented at `ChangeOwner` old-owner side (`0x00448AC2`) and `Limbo` (`0x00445925`), all gated on `Type+0x16CC`. Not a "storage facility count" (corrected 2026-09-06). | HIGH (binary) |
| `+0x54E8` | int | Credits — likely the live/authoritative value | MEDIUM (written by Add_Tiberium_Credits) |
| `+0x5398` | int | Accumulated OrePurifier bonus from `RecalcBonuses` case 3 (input to GetAccumulatedBonus) | MEDIUM (Agent D citation; not re-verified — see Open Q §7.6) |

### RulesClass

| Offset | Type | INI key | YR default | Active path |
|--------|------|---------|------------|-------------|
| `+0xF3C` | float | `PurifierBonus=` | 0.25 | Both Allied/Soviet (Mission_Deploy_Building) and Yuri (DepositOreFromStorage) deposit paths |
| `+0x1324` | `int*` (3 ints) | `AIVirtualPurifiers=` | `{4, 2, 0}` | Both deposit paths (AI gating) |
| `+0x1528` | double | `HarvesterDumpRate=` (in *minutes*) | `0.016` (= 14.4 frames at 15fps × `_DAT_007E27F8 = 900.0`) | Mission_Deploy_Building state 3 gate |
| `+0x1700` | double | `ConditionYellow` threshold (health ratio) | (engine constant) | Both anim-slot selection paths (damaged vs healthy variant) |
| `+0x17D0` | int | (TS-legacy `TotalStorage` — global hard cap) | (unknown) | Only the Add_Tiberium_To_Storage path (Weeder-only) |

### Globals

| Address | Value | Purpose |
|---------|-------|---------|
| `0x7E1748` (FLOAT_007e1748) | `0.0f` | Epsilon for "is slot non-empty?" and "did drain happen?" |
| `0x7E27F8` (DAT_007e27f8) | `900.0` | 60 sec × 15 fps — multiplier for `HarvesterDumpRate × 900 ≤ counter` gate |
| `g_GameMode` | int | Non-zero in active skirmish (gates AIVirtualPurifiers add) |
| `g_RulesClass_Instance` | `RulesClass*` | Global rules instance |

## 3. Core Logic

### 3a. Allied/Soviet harvester deposit (per-fire drain)

Inside `UnitClass::Mission_Deploy_Building` state 3 (decompiled at `0x73D630`, drain block roughly `0x73E2C0 – 0x73E539`):

```pseudo
function Mission_Deploy_Building_state3(harvester):
    if HarvesterDumpRate * 900.0 > harvester[+0xF8]:
        return 1  // not yet time to drain — counter still accumulating

    refinery = LookupBuildingInCell(harvester.position + cell_offset)
    if refinery is null: return 1

    refinery.vtable[+0x468]()   // post-deposit virtual callback (purpose TBD — see Open Q §7.7)

    if refinery[+0x584] == 0:   // SpecialAnim slot 10 not currently playing
        SetAnimSlotImage(slot=10, damaged = (health_ratio <= ConditionYellow), 0, 0)
        // ↑ This is the per-bale flash (already implemented in our Rust)

    slot_index = harvester.StorageClass.FindFirstNonEmptySlot()
    refinery_owner = refinery.vtable[+0x3C]()    // get HouseClass*
    facility_count = refinery_owner[+0x538C]

    if !refinery_owner.IsHuman and g_GameMode != 0:
        facility_count += AIVirtualPurifiers[refinery_owner[+0x184]]   // {4, 2, 0}[difficulty]

    if slot_index == -1:
        amount = 0
    else:
        amount = harvester.StorageClass.GetAmount(slot_index)   // FULL slot value

    bonus = facility_count * Rules.PurifierBonus * amount

    if slot_index != -1:
        // Drain the entire slot in one call
        drained = harvester.StorageClass.RemoveAmount(amount, slot_index)
        if drained > 0.0:
            if harvester.UnitType[+0xE0F] == 0:   // standard path (NOT Weeder)
                refinery_owner.Add_Tiberium_Credits(drained, slot_index)
                if bonus > 0:
                    refinery_owner.Add_Tiberium_Credits(bonus, slot_index)
                harvester[+0xF8] = 0   // reset dump counter
            else:
                // TS-legacy Weeder path — NEVER REACHED in standard YR
                Add_Tiberium_To_Storage(ftol(drained), slot_index)
                harvester[+0xF8] = 0
            goto post_drain

    // Slot was empty (or drained to zero) — finish unloading
    if refinery.Type.Refinery:    // +0x16BB
        SetAnimSlotImage(slot=8, damaged = (health_ratio <= ConditionYellow), 0, 0)
    harvester.state = 4   // → depart
    if refinery[+0x584] != 0:
        refinery.ClearAnimSlot(slot=10)   // stop the per-bale flash

    post_drain:
        if non-trivial conditions: also transition to state 4 + clear slot 10
        return 1
```

**Key tiny details:**
- The drain is per-SLOT, not per-bale. `RemoveAmount(amount, slot)` is called with `amount = GetAmount(slot)` — i.e., the entire slot is drained in a single call.
- The post-drain Add_Tiberium_Credits doublecall: first the base amount, then the bonus. The bonus check `bonus > FLOAT_007E1748` (= 0.0f) — bonus only awarded if non-zero.
- The dump counter (`+0xF8`) is reset to 0 only on successful drain. Gate fires every `ceil(HarvesterDumpRate × 900)` = `ceil(14.4)` = 15 frames (assuming counter increments by 1/frame; see Open Q §7.2).
- The state transition to 4 (depart) happens when `FindFirstNonEmptySlot` returns -1 (all slots empty) AFTER drain — *not* on the same fire as the last drain. The harvester's drain → wait 15 frames → next fire returns nothing → state 4.
- SpecialAnim slot 10 is gated on `BuildingClass+0x584 == 0` — only triggers if no slot-10 anim is already playing. The "is it playing" check is the binary's mechanism for once-per-bale firing. This matches our existing `consume_bale_events` Rust impl.
- Slot 8 anim fires on the *empty* iteration (transition to state 4), not on the drain. Slot 7 anim fires earlier in the state machine (state 1 → 3 transition). Slot 10 fires per drain.

### 3b. Yuri Slave Miner deposit (drain-on-arrival)

Inside `BuildingClass::DepositOreFromStorage` (decompiled at `0x522D50`). **Note (corrected 2026-09-06):** `ECX` (this) is the arriving *slave*, not the building — `LEA EBP,[ECX+0x33C]` at `0x00522D55` addresses the slave's own `StorageClass`; the building is the stack argument (`[ESP+0x20]`), used only for `+0x21C` owner and the `vtable+0x468` smoke callback:

```pseudo
function DepositOreFromStorage(slave /*ECX*/, building /*stack arg*/):
    any_credits_awarded = false
    slot = slave.StorageClass.FindFirstNonEmptySlot()   // slave+0x33C

    while slot != -1:
        owner = building.Owner    // building[+0x21C]
        facility_count = owner[+0x538C]
        if !owner.IsHuman and g_GameMode != 0:
            facility_count += AIVirtualPurifiers[owner[+0x184]]

        amount = slave.StorageClass.GetAmount(slot)
        bonus = facility_count * Rules.PurifierBonus * amount

        drained = slave.StorageClass.RemoveAmount(amount, slot)
        if drained > 0.0:
            any_credits_awarded = true
            building.Owner.Add_Tiberium_Credits(drained, slot)
            if bonus > 0:
                building.Owner.Add_Tiberium_Credits(bonus, slot)

        slot = slave.StorageClass.FindFirstNonEmptySlot()

    if any_credits_awarded:
        building.vtable[+0x468]()   // post-deposit callback
    return
```

**Called from:** `SlaveManagerClass::AI_Update` state 4 (`0x6AFBD2` at offset `0x6AFC02` based on decompile context). When a slave infantry reaches the dock cell of the deployed Slave Miner refinery (param_1+0x24 = the building), this function drains the arriving slave's entire StorageClass (slave+0x33C) to the building owner's credits in one synchronous call.

**Key tiny details:**
- Unlike the Allied/Soviet path, this loops over ALL slots in one call (the `while` loop). The Allied/Soviet path drains one slot per fire of the dump-counter gate, so multi-slot harvesters take multiple ~15-frame fires.
- The post-deposit callback (`vtable+0x468`) fires once, after all drains complete — not per slot.
- There is no slave deposit *into* the building: `FUN_00522E70` adds harvested ore to the slave's own `+0x33C` storage (`LEA EDI,[ESI+0x33C]` at `0x00522F0A`, `ESI` = slave) — see Open Q §7.8 (resolved).

### 3c. Storage-tier visual indicator (already verified prior session)

Inside `BuildingClass::UpdateAnimation` phase F (`0x4509D0`, gated on `Type+0x16BB == Refinery=yes`):

```pseudo
function UpdateAnimation_phaseF(building):
    if !building.Type.Refinery: return

    stored = building.StorageClass.GetTotalAmount()
    if stored == 0:
        new_tier = 0
    else:
        new_tier = floor((stored * 4) / building.Type.Storage)
        // tier 0: 0% – 25%        → ActiveAnim     (slot 3, GAREFNL1)
        // tier 1: 25% – 50%       → ActiveAnimTwo  (slot 4, GAREFNL2)
        // tier 2: 50% – 75%       → ActiveAnimThree(slot 5, GAREFNL3)
        // tier ≥ 3: 75% – 100%+   → ActiveAnimFour (slot 6, GAREFNL4)

    cached_tier = building[+0x6F0]
    if cached_tier != new_tier:
        ClearAnimSlot(prior tier's slot 3/4/5/6)
        building[+0x6F0] = new_tier
        anim_field_offset = (new_tier == 0 ? +0x1018 : new_tier == 1 ? +0x105C : new_tier == 2 ? +0x10A0 : +0x10E4)
        damaged_offset    = (anim_field_offset + 0x10)
        anim_name = (health_ratio <= Rules.ConditionYellow) ? building.Type[damaged_offset] : building.Type[anim_field_offset]
        if anim_name != null and anim_name[0] != '\0':
            CreateAnimForSlot(slot=new_tier+3, anim_name, ...)
```

**Source of `stored`:** The Allied/Soviet refinery's `StorageClass` is **never written** by the standard harvester path (see §3a). So in stock YR play, GAREFN/NAREFN refineries always have `stored == 0` and remain stuck at tier 0. Yuri's Slave Miner building's StorageClass is likewise *not* written by slave deposits — slaves carry ore in their own `+0x33C` storage and `0x00522D50` drains the slave, not the building (corrected 2026-09-06).

**For our Rust port, this means the tier indicator cannot be driven from the existing bale → credits flow — it needs a separate per-refinery "display counter" that ticks on each `BaleDepositEvent`.** See §6 below.

## 4. Per-Fire Drain Is Whole-Slot (parity finding — NEW)

> **Decided in Batch A checkpoint** that this finding belongs in the report. It's not in any prior gap-scan.

**Claim:** gamemd drains the harvester's *entire slot 0* (ore) in **one** ~15-frame dump fire. A fully-loaded War Miner (40 ore in slot 0) unloads in ~15 frames total at the refinery — not 40 × 15 = ~600 frames as our current Rust impl does.

**Evidence:**
- `Mission_Deploy_Building` state 3 (binary, decompiled): `amount = GetAmount(slot)` (full slot value) → `RemoveAmount(amount, slot)` (drains it all) → `Add_Tiberium_Credits(amount, slot)` (awards it all).
- Inside one call to `Mission_Deploy_Building`, exactly one slot is drained, then the function returns. Next call (~next frame), the counter has been reset to 0; another 15-frame wait, then either the next non-empty slot drains or the harvester transitions to state 4 (depart).
- For a single-slot harvester (typical — 40 ore, 0 gems), this gives **one drain + 1 idle wait + state-4 transition = ~30 frames total** at the refinery, not 600.

**How our current Rust diverges:**
- Our `Vec<CargoBale>` model treats each bale as an independent discrete unit.
- `phase_unloading` decrements `unload_timer` by 10/tick and pops one `CargoBale` per gate fire.
- For 40 bales × ~15 frames each = ~600 frames at the refinery. **~40× slower than gamemd.**

**Player-visibility:** Highly visible. A harvester sits at a gamemd refinery for ~2 seconds (30 frames at 15fps) but at our refinery for ~40 seconds. This is the kind of "everything feels slow" parity drift CLAUDE.md warns about.

**Confidence:** HIGH. Decompile-verified, both code paths (Allied/Soviet inline + Yuri DepositOreFromStorage) drain whole-slot — there's no per-bale drain anywhere in the binary.

**Active in YR:** Yes — every harvester unload, every match.

**Recommended follow-up:** A separate `/brainstorm refinery unload cadence` to design either (a) Rust drains the full Vec<CargoBale> in one tick of the gate, or (b) the cargo model itself collapses to a continuous "amount" per resource type. Either reproduces gamemd's observable timing.

## 5. PurifierBonus Formula (count-based, not boolean)

**The formula** (identical in both Mission_Deploy_Building and DepositOreFromStorage):

```
facility_count = REFINERY_OWNER[+0x538C]
if !REFINERY_OWNER.IsHuman and g_GameMode != 0:
    facility_count += AIVirtualPurifiers[REFINERY_OWNER[+0x184]]
bonus_credits = facility_count * Rules.PurifierBonus * drained_amount
```

Then **two** `Add_Tiberium_Credits` calls:
1. `Add_Tiberium_Credits(drained_amount, slot)` — base credits (always, if drained > 0)
2. `Add_Tiberium_Credits(bonus_credits, slot)` — bonus credits (only if `bonus > 0.0`)

**Tiny details:**
- The multiplier is `(float)facility_count`, not `(facility_count > 0 ? 1 : 0)`. **One purifier = +25%, two purifiers = +50%, etc.** (with `PurifierBonus=0.25`).
- The `IsHuman` check on the **refinery's** owner — not the harvester's owner. Harvester ownership is irrelevant to the bonus.
- The `g_GameMode != 0` gate suggests the AIVirtualPurifiers add only applies in skirmish/campaign play, not in main-menu/preview states.
- The bonus is added as a **separate** `Add_Tiberium_Credits` call — so the cached/display field (HouseClass+0x30C/+0x54E8) is updated twice per drain. This may matter for any UI that watches credits change-events.

**Current Rust divergence (gap-scan #16):** [src/sim/miner/miner_dock_sequence.rs:386-393](src/sim/miner/miner_dock_sequence.rs#L386-L393) uses a boolean `player_has_purifier()` check × `purifier_bonus_pct/100`. This applies the bonus on `purifier_bonus_pct/100 = 25%` whenever any purifier is owned. Gap-scan-confirmed; count-based fix required.

**Confidence:** HIGH. Both code paths verified.

**Active in YR:** Yes — every deposit, every match.

## 6. AIVirtualPurifiers (AI-difficulty bonus)

**Storage:** `RulesClass+0x1324` is a pointer to a 3-int array (default `{4, 2, 0}`).

**Indexing:** `REFINERY_OWNER[+0x184]` — the owner's *difficulty index*. **Resolved 2026-05-19: order is `Hard=0, Medium=1, Easy=2`.** Evidence:

- `HouseClass::SetDifficulty @ 0x004F6EC0` writes `param_2` directly to `HouseClass+0x184`; the skirmish AI-house construction path passes 0, 1, 2 in Brutal/Medium/Easy order. Verified via `decompile_function 0x004F6EC0` (see [ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md) slot-4 of the 2026-05-19 miner-docking swarm).
- `ini/rulesmd.ini:89` carries an inline comment that is definitive: `AIVirtualPurifiers=4,2,0 ; h,m,e` — i.e., Hard=4, Medium=2, Easy=0.

Brutal AI (`index=0`) gets +4 virtual purifiers → bonus = `4 × 0.25 × amount = +100%` on every bale. Doubles AI ore income.

**Tiny details:**
- The AI bonus is added to `facility_count` BEFORE the bonus calc — so it scales with `PurifierBonus`, not as a flat multiplier.
- Only active when (`!IsHuman` AND `g_GameMode != 0`). Pre-game / menu / replay-paused contexts skip the bonus.
- Each owned real Ore Purifier (set via `OrePurifier=yes` on the building type) is presumably already counted in `HouseClass+0x538C` via `RecalcBonuses` — so an AI player with both real purifiers AND virtual purifiers stacks the bonus.

**Current Rust:** **ABSENT.** [src/sim/game_options.rs:49-50](src/sim/game_options.rs#L49-L50) has an `ai_difficulty: i32` field but it's read by zero gameplay code. The whole AI-bonus path is missing.

**Confidence:** HIGH on formula and mechanism. HIGH on the index → difficulty mapping (Open Q §7.5 resolved 2026-05-19).

**Active in YR:** Yes — every AI player's deposit, every match.

## 7. INI Keys

| Key | Section | YR Default | Active Path | Currently Parsed in Rust? |
|-----|---------|------------|-------------|----------------------------|
| `Storage=` | `[GAREFN]`/`[NAREFN]`/`[YAREFN]` | 200 | Phase F tier-display denominator | YES — `obj.storage` |
| `Refinery=` | `[GAREFN]`/`[NAREFN]` | yes | Phase F gate | YES — `obj.refinery` |
| `Refinery=` | `[YAREFN]` | **absent** | — | n/a (YAREFN has no tier visual) |
| `Storage=` | `[HARV]`/`[CMIN]`/`[SMIN]` | 40/20/0 | Cap in Harvest_Ore_Tick | YES — `obj.storage` |
| `OrePurifier=` | `[GAOREP]`/`[NAOREP]` | yes | Contributes to HouseClass+0x538C via RecalcBonuses | YES — `obj.ore_purifier` |
| `PurifierBonus=` | `[General]` | 0.25 | Both deposit paths (multiplier) | YES — `rules.general.purifier_bonus_pct` (currently treated as boolean × pct; needs count-based refactor) |
| `AIVirtualPurifiers=` | `[General]` | 4,2,0 | Both deposit paths (AI-difficulty extra count) | **NO — absent in Rust** |
| `HarvesterDumpRate=` | `[General]` | 0.016 (minutes) | Mission_Deploy_Building state 3 gate (× 900 = 14.4 frames) | YES — `rules.general.harvester_dump_tenths` (× 10 storage encoding) |
| `Weeder=` | `[<UnitType>]` | absent on standard harvesters | Routes deposit to Add_Tiberium_To_Storage instead of credits — TS-LEGACY | NOT parsed (correctly — TS-legacy) |
| `RefinerySmokeOffset{One,Two,Three,Four}=` | per-building | varies | consume_bale_events particle spawn | YES — emitted from `consume_bale_events` |
| `RefinerySmokeParticleSystem=` | per-building | SmallGreySSys | particle type for smoke | YES |
| `SpecialAnim=` on refineries | per-building | varies | slot-10 per-bale flash | YES — triggered by `consume_bale_events` |
| `[GAREFNL1..L4]`/`[NAREFNL1..L4]` | `[Animations]` | (built-in art) | Tier visual SHP per tier | Loaded via art registry; needs tier-driven selection per §3c |

## 8. Integration Points

### Callers (verified)

| Caller | Calls | When | Path |
|--------|-------|------|------|
| `BuildingClass::Update @ 0x43FB20` (vtable per-frame) | `BuildingClass::UpdateAnimation @ 0x4509D0` | Every frame, every building | Tier visual |
| `UnitClass::Mission_Deploy_Building @ 0x73D630` state 3 | `Add_Tiberium_Credits` × 2 (base + bonus), `Add_Tiberium_To_Storage` (Weeder only) | Per dump-counter fire (~15 frames) during harvester unload | Allied/Soviet credit flow |
| `SlaveManagerClass::AI_Update @ 0x6AFBD2` state 4 | `DepositOreFromStorage` on building+0x24 (Slave Miner refinery) | When slave reaches dock cell with cargo | Yuri credit flow |
| `BuildingClass::Sell` | `Add_Tiberium_Credits` | Building sell refund | Refund (out of scope) |
| `UnitClass::Harvest_Ore_Tick @ 0x73D450` | `StorageClass::AddAmount` × 2 | Per harvest cycle | Harvester storage fill (in scope of multi-bale work just shipped) |
| `FUN_00522E70` | `StorageClass::AddAmount` | Slave harvest into the slave's own `+0x33C` storage (`0x00522F0A`) | Yuri half (Open Q §7.8 resolved) |
| `HouseClass::Add_Tiberium_To_Storage @ 0x4F9700` | `StorageClass::AddAmount` × N | Weeder loop (TS-legacy, never reached) | Dormant |

### Tick-order (binary)

`BuildingClass::Update` runs every frame for every building, calling `UpdateAnimation` which reads `StorageClass::GetTotalAmount`. So the tier-visual would update within one frame of any change to a refinery's StorageClass. Mission_Deploy_Building runs every frame per harvester in deploy mission. There's no observable ordering hazard within a single frame for credits vs visuals.

## 9. Current Rust Implementation Status

| Feature | Rust status | Gap | Action |
|---------|-------------|-----|--------|
| Bale → credits flow (basic) | [src/sim/miner/miner_dock_sequence.rs:381](src/sim/miner/miner_dock_sequence.rs#L381) | Works for the *output* (credits arrive). Cadence is per-bale, gamemd is per-slot. | Per §4 — separate brainstorm |
| Per-bale SpecialAnim slot 10 | [src/app_building_anim.rs:341](src/app_building_anim.rs#L341) `consume_bale_events()` | Already implemented | None — gap-scan #5 was wrong |
| Per-bale smoke particles (RefinerySmokeOffsetN) | Same file as above | Already implemented | None — gap-scan #6 was wrong |
| First-bale waits one full interval | [src/sim/miner/miner_dock_sequence.rs](src/sim/miner/miner_dock_sequence.rs) phase_linked | Just shipped (2026-05-12) | None — done |
| PurifierBonus count-based | [src/sim/miner/miner_dock_sequence.rs:386-393](src/sim/miner/miner_dock_sequence.rs#L386-L393) | Currently boolean — needs count | Brainstorm + implement |
| AIVirtualPurifiers AI-bonus | (absent) | Field exists in `GameOptions.ai_difficulty` but unread | Add parser for INI key, wire into deposit |
| Refinery storage-tier visual | [src/app_instances/shp.rs:508](src/app_instances/shp.rs#L508) | All 4 ActiveAnim slots draw stacked | Add display-only "stored counter" on per-refinery component; on `BaleDepositEvent`, increment by bale value; render only tier slot per `(stored × 4) / Storage` |
| Per-fire drain is whole-slot | (per-bale cadence) | ~40× slower than gamemd | Brainstorm + implement |
| Yuri Slave Miner deposit | Not yet implemented (Yuri faction WIP) | n/a | Out of scope until Yuri faction work begins |
| Weeder-path Add_Tiberium_To_Storage | (absent) | TS-legacy, no fix needed | None |

## 10. Gap-scan Corrections

The current `docs/gap-scans/2026-05-12-gap-scan-miner-deep.md` lists 11 detail-drift findings. After this investigation:

- **#5 (per-bale SpecialAnim not played)** — **WRONG, already implemented.** [src/app_building_anim.rs:341](src/app_building_anim.rs#L341) does it. Should be re-marked DONE.
- **#6 (per-bale smoke particles not emitted)** — **WRONG, already implemented.** Same file. Should be re-marked DONE.
- **#16 (PurifierBonus formula wrong — count vs bool)** — **CONFIRMED.** Formula is `count × bonus × amount`, not `bool ? bonus : 0`.
- **#8 (AI difficulty credit bonus missing)** — **CONFIRMED.** AIVirtualPurifiers indexed by HouseClass+0x184.
- **#7 (Refinery storage-tier ActiveAnim wrong)** — **CONFIRMED.** Phase F tier-gating verified previous session; this report verifies the tier formula precisely (`(stored × 4) / Storage`) and identifies the missing input (per-refinery display counter).
- **NEW finding (not in any gap-scan): per-fire drain is whole-slot** — see §4. Approximately 40× cadence drift.

## 11. Open Questions

7.1 **Field_0xF8 increment site.** I confirmed the counter at `UnitClass+0xF8` is reset to 0 after each drain and gated by `HarvesterDumpRate × 900 ≤ counter`. I did not find the per-frame incrementer — likely in `UnitClass::Update`, `TechnoClass::Update`, or a missionclass step helper. **Effect on Rust port:** assume 1 increment per frame; verify if cadence still drifts after fix.

7.2 **Is the dump counter in frames or in some other unit?** `HarvesterDumpRate × 900 = 14.4` and `900 = 60 sec × 15 fps`, so it's almost certainly "frames since reset". Verify by tracing 7.1.

7.3 **HouseClass+0x30C vs +0x54E8.** Both written by `Add_Tiberium_Credits` with the same `ftol(amount)`. Likely one is live counter, other is cached/UI display. Identify by finding readers — UI code reads one, gameplay code reads the other. Not critical for parity (we maintain only one credit field in Rust).

7.4 **Add_Tiberium_To_Storage host.** The decompile shows it calls `StorageClass::AddAmount` and `StorageClass::GetTotalAmount` without an obvious `this` argument visible at this level. Likely it operates on a global/static StorageClass (TS-era `HouseClass::Storage` field?). Since this path is dead in YR, identifying the host doesn't matter for parity — flagging only.

7.5 **AIDifficulty index order — RESOLVED 2026-05-19.** Order is `{Brutal=0, Medium=1, Easy=2}` — Brutal AI gets `{4,2,0}[0] = 4` virtual purifiers (+100% ore income). Verified via `HouseClass::SetDifficulty @ 0x004F6EC0` decompile + `rulesmd.ini:89` inline comment `AIVirtualPurifiers=4,2,0 ; h,m,e`. See [ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ADD_TIBERIUM_CREDITS_PURIFIER_VIRTUAL_PURIFIERS_GHIDRA_REPORT.md).

7.6 **HouseClass+0x538C vs +0x5398 relationship — RESOLVED 2026-09-06.** `+0x538C` is the OrePurifier *building count*: `INC [Owner+0x538C]` at `OnConstructionComplete` `0x0044637C` and `ChangeOwner` `0x004491EB`, `DEC` at `ChangeOwner` `0x00448AC2` and `Limbo` `0x00445925`, each gated on `Type+0x16CC` (`OrePurifier=`, bound at `BuildingTypeClass::ReadINI` `0x004604ED`/`0x004604FA`). It is not a storage-facility count and is not written by `RecalcBonuses`.

7.7 **vtable+0x468 callback.** Fires after each Mission_Deploy_Building drain AND once at the end of DepositOreFromStorage. Slot offset 0x468 = vtable index `0x468/4 = 0x11A`. Likely a "refresh display / mark dirty / play deposit sound" virtual. Identify by finding the slot in `BuildingClass` vtable at `0x007E3F18`.

7.8 **`FUN_00522E70` slave deposit — RESOLVED 2026-09-06.** Calls `StorageClass::AddAmount` on the *slave's own* storage (`LEA EDI,[ESI+0x33C]` at `0x00522F0A`, `ESI` = slave/this), not the Slave Miner building. `0x00522D50` then drains that same slave storage (`LEA EBP,[ECX+0x33C]` at `0x00522D55`, `ECX` = slave); the building argument supplies only `+0x21C` owner and `vtable+0x468`.

## Sources

- **Ghidra MCP** — gamemd.exe loaded at instance `127.0.0.1:8089`.
- **Functions fully decompiled:** `0x522D50`, `0x4F9610`, `0x6C9650`, `0x6C9690`, `0x6C96B0`, `0x6C9820`, `0x4F9700`, `0x73D630`, `0x6AFBD2`.
- **Function spot-checked:** `0x4509D0` (verified previous session).
- **Strings looked up:** `Weeder` @ `0x81AC50` (xref → UnitTypeClass::ReadINI at `0x7476C0`), `Difficulty` strings (3 difficulty names: `BRUTAL/MEDIUM/EASY`).
- **Memory reads:** `0x7E1748` = `0.0f` (epsilon).
- **Byte-pattern searches:** `0F 0E 00 00` (offset 0xE0F), `D0 17 00 00` (offset 0x17D0).
- **Related docs (cross-referenced):**
  - [docs/plans/2026-05-12-refinery-storage-flow-investigation-plan.md](docs/plans/2026-05-12-refinery-storage-flow-investigation-plan.md) — this investigation's plan
  - [docs/gap-scans/2026-05-12-gap-scan-miner-deep.md](docs/gap-scans/2026-05-12-gap-scan-miner-deep.md) — findings #5, #6, #7, #8, #16 cross-referenced
  - [BUILDINGCLASS_UPDATE_ANIMATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGCLASS_UPDATE_ANIMATION_GHIDRA_REPORT.md) — phase F (tier visual) — verified
  - [ORE_VALUE_CREDIT_DEPOSIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/ORE_VALUE_CREDIT_DEPOSIT_GHIDRA_REPORT.md) — StorageClass struct — verified
  - [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) — RulesClass offsets — partially verified (Open Q §7.5/§7.6 leave some unresolved)
- **INI files checked:** `ini/rulesmd.ini`, `ini/rules.ini`, `ini/artmd.ini`, `ini/art.ini`.
