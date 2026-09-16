---
title: RulesClass [Powerups] crate-bonus table schema
source_addr: 0x00673E80
owner_report: [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) §5 (Master orchestrators, step 32)
yr_active_in_stock_game: YES
writes_to_rulesclass: NO (writes to 4 parallel globals, not a RulesClass field)
verified_from: gamemd.exe live decompilation (Ghidra MCP, 2026-04-24); token-3 semantics, flag-array width, default string, and static defaults re-verified 2026-09-02 against the live binary; cross-checked against ini/rulesmd.ini §[Powerups]
---

# `[Powerups]` crate-bonus table

## Summary

`FUN_00673E80` ("ReadPowerups") parses the `[Powerups]` section of
`rulesmd.ini` into **four parallel globals** — NOT into the RulesClass
instance. It runs as step 32 of the RulesClass dispatcher
(`FUN_00668BF0`), but the data it writes lives at fixed addresses outside
`RulesClass*`. Consumers are `CrateClass::PickupDispatch` (crate pickup at
runtime) and the save/load routines at `FUN_0067F7E0` / `FUN_0067F9C0`.

INI line format:

```
<PowerupName>=<weight>, <anim>, <over-water>, <value>
; e.g. Money=20,MONEY,yes,2000
;      Armor=10,ARMOR,yes,1.5
;      Napalm=0,<none>,no,600
```

- `weight` — int, crate-drop weight used for random powerup selection
- `anim` — string, matches an `AnimType` name (`<none>` → `-1` via the
  `AnimTypeClass::Find_Index` helper at `FUN_00422B20`)
- `over-water eligibility` — `yes`/`no`. **NOT an "enabled" flag.** Verified
  2026-09-02 at `CrateClass__PickupDispatch 0x00481D52`: the byte is read only
  when the crate cell's land type is water (`CMP dword [ESI+0xEC], 0x2`), and a
  cleared flag redirects the outcome to slot 0 (Money) rather than suppressing
  the crate. Stock `Unit=20,<none>,no` therefore still drops on land — the older
  "enabled" reading would have forbidden that. Anything other than the two
  literal strings leaves the flag at its previous value.
- `value` — double, the powerup's per-kind effect parameter. If the token
  contains a `%`, the raw `atof` is multiplied by `0.01` (so `50%` becomes
  `0.5`); otherwise it is stored verbatim. Interpreted per-powerup:
  - Money → the **minimum** cash granted, not the maximum. The Money branch
    does `EAX = ftol(magnitude); EDX = EAX + 0x384; RandomRanged(EAX, EDX)`
    (`0x00482484..0x004824A0`), so stock `Money=20,MONEY,yes,2000` pays
    **2000–2900**. The retail INI's own trailing comment `(maximum cash)` is
    wrong and this doc inherited it. The solo override is narrower than it
    looks: `PickupDispatch` loads `SoloCrateMoney` only when `g_GameMode == 0`
    **and** the cell's overlay data byte is zero, and the draw is then skipped
    only when that loaded value is non-zero — a `SoloCrateMoney=0` still rolls
    the random amount.
  - Armor/Firepower/Speed → multiplier applied to nearby units
  - Veteran → veteran levels added
  - Invulnerability → duration in minutes
  - Explosion/Napalm/Gas → damage per explosion
  - Unused entries → ignored

## Fixed 19-slot layout

The table is **not** INI-ordered. Every slot is a fixed index into the
name-pointer array at `0x007E523C`. The N-th slot in each of the four
parallel globals corresponds to the N-th name in this list:

| Idx | Powerup Name (ASCII literal) | Name pointer |
|---:|---|---|
| 0 | `Money` | `0x0081DA20` |
| 1 | `Unit` | `0x0081746C` |
| 2 | `HealBase` | `0x0081DA14` |
| 3 | `Cloak` | `0x0081DA0C` |
| 4 | `Explosion` | `0x0081DA00` |
| 5 | `Napalm` | `0x0081D9F8` |
| 6 | `Squad` | `0x0081D9F0` |
| 7 | `Darkness` | `0x0081D9E4` |
| 8 | `Reveal` | `0x0081D9DC` |
| 9 | `Armor` | `0x0081D9D4` |
| 10 | `Speed` | `0x0081D9CC` |
| 11 | `Firepower` | `0x0081D9C0` |
| 12 | `ICBM` | `0x0081D9B8` |
| 13 | `Invulnerability` | `0x0081D9A8` |
| 14 | `Veteran` | `0x0081D9A0` |
| 15 | `IonStorm` | `0x0081D994` |
| 16 | `Gas` | `0x0081D990` |
| 17 | `Tiberium` | `0x00817278` |
| 18 | `Pod` | `0x0081D98C` |

Total: **19 slots**. Every INI row in stock `rulesmd.ini` `[Powerups]`
maps to exactly one of these (grep-verified, 19 non-blank entries in
the `[Powerups]` section).

## Four parallel global arrays

Each has **exactly 19 entries**, layout confirmed by the loop bound in
`FUN_00673E80`:

| Global base | Array type | Size (bytes) | Field source | Semantics |
|---|---|---:|---|---|
| `DAT_0081DA8C` | `int32[19]` | `0x4C` (76) | 1st token, via `atoi` | **drop weight** — summed across **all nineteen** slots for random selection; a zero weight is the only thing that makes a slot unrollable |
| `DAT_0081DAD8` | `int32[19]` | `0x4C` (76) | 2nd token, via `AnimTypeClass::Find_Index` (`FUN_00422B20`) | **pickup anim index** — index into `g_AnimTypes_Array`, or `-1` for `<none>` |
| `DAT_0089ECC0` | `u8[19]` | `0x13` (19) | 3rd token, literal strcmp | **over-water eligibility** — `1` if `yes`, `0` if `no`, otherwise unchanged. Byte-wide: the writes at `0x00673F64`/`0x00673F7F` are `MOV byte ptr [EDI + 0x89ecc0], ...` and the read at `0x00481D5B` is `MOV AL, byte ptr [EBX + 0x89ecc0]` |
| `DAT_0089EC28` | `double[19]` | `0x98` (152) | 4th token, via `atof` (× `0.01` if `%` present) | **effect parameter** — see per-powerup table above |

`DAT_0081DA8C` and `DAT_0081DAD8` are contiguous (`0x0081DA8C..0x0081DB24`)
— a single `int32[38]` area split logically in two. `DAT_0089EC28` and
`DAT_0089ECC0` are also contiguous: `double[19]` (`0x0089EC28..0x0089ECC0`)
immediately followed by `u8[19]` (`0x0089ECC0..0x0089ECD3`). The `f64` cursor's
loop bound `pdVar5 < 0x0089ECC0` is exactly the start of the flag array.

## Static image defaults (read 2026-09-02)

The four globals are initialized in the image, not by the RulesClass
constructor — `get_xrefs_to` on each base shows only ReadPowerups and the
save/load pair. Pre-INI values:

| Global | Default |
|---|---|
| `DAT_0081DA8C` weights | `[50,20,1,3,5,5,20,1,1,10,10,10,1,3,1,1,1,1,1]` |
| `DAT_0081DAD8` anim | all `-1` |
| `DAT_0089ECC0` over-water | all `0` |
| `DAT_0089EC28` magnitude | all `0.0` |

## Function body

```c
undefined4 FUN_00673E80(void) {
    if (CCINIClass__Find_Section("Powerups") == 0) return 0;

    int     iVar1  = 0;
    double* pdVar5 = (double*)&DAT_0089EC28;
    char    local_80[128];

    do {
        if (CCINIClass__ReadString("Powerups",
                                   (&PTR_s_Money_007E523C)[iVar1],   // name from fixed table
                                   "0,NONE",                         // default string @ 0x0083D4AC
                                   local_80, 0x80) != 0) {
            // Field 1: drop weight (atoi)
            char* tok = CRT__strtok(local_80, ",");
            if (tok) { strtrim(); (&DAT_0081DA8C)[iVar1] = atoi(tok); }

            // Field 2: anim name → AnimType index
            tok = CRT__strtok(0, ",");
            if (tok) { strtrim(); (&DAT_0081DAD8)[iVar1] = AnimTypeClass::Find_Index(tok); }

            // Field 3: over-water eligibility
            tok = CRT__strtok(0, ",");
            if (tok) {
                strtrim();
                if      (strcmpi(tok, "yes") == 0) *(u8*)(&DAT_0089ECC0 + iVar1) = 1;
                else if (strcmpi(tok, "no")  == 0) *(u8*)(&DAT_0089ECC0 + iVar1) = 0;
                // else leave previous value untouched
            }

            // Field 4: effect parameter (optionally %-scaled)
            tok = CRT__strtok(0, ",");
            if (tok) {
                bool is_percent = (strchr(tok, '%') != 0);
                if (!is_percent) strtrim();
                double v = atof(tok);
                if (is_percent) v *= 0.01;
                *pdVar5 = v;
            }
        }

        pdVar5 += 1;
        iVar1  += 1;
    } while ((int)pdVar5 < 0x0089ECC0);
    return 1;
}
```

## Consumers (verified xrefs)

- **`CrateClass::PickupDispatch` at `0x00481A90`** — crate-pickup
  dispatcher. Reads all four globals, indexed by the resolved powerup
  type. Key call sites:
  - `0x00481ADA` — `MOV ECX, 0x81DA8C; ... ADD EAX, dword ptr [ECX];
    ADD ECX, 0x4; CMP ECX, 0x81DAD8` → sums `DAT_0081DA8C` weights across
    all 19 slots for weighted random selection.
  - `0x00481B06` — same `DAT_0081DA8C` base, secondary pass that compares
    a running sum against a previously-generated random value to pick the
    winning slot.
  - `0x00481D52..0x00481D67` — `CMP dword ptr [ESI+0xEC], 0x2; JNZ skip;
    MOV AL, byte ptr [EBX + 0x89ECC0]; TEST AL, AL; JNZ skip; XOR EBX, EBX;
    MOV [ESP+0x2C], EBX` → **only on a water cell**, a cleared flag rewrites
    the selected slot to 0 (Money). It does not disable the powerup.
  - `0x00481DC3` — `FLD double ptr [EBX*0x8 + 0x89EC28]` → loads the
    effect parameter as a double for application.
- **Save/load at `0x0067F7E0` and `0x0067F9C0`** — read/write all four
  globals (likely scenario init fallback / save-game serialisation).
- `Layer_From_Name @ 0x0048E074` appears in the xref list for
  `DAT_0081DA8C` but is a stray offset; the bytes at `0x0081DA8C` are
  also near string-pool content and Ghidra sometimes flags adjacent
  pointers.

## YR-active status — **live**

Every entry is reachable from crate pickup at runtime, but **weight alone
decides what is rolled**. Exactly eight stock slots carry a positive weight —
`Money` 20, `Unit` 20, `HealBase` 10, `Reveal` 10, `Armor` 10, `Speed` 10,
`Firepower` 10, `Veteran` 20 — totalling **110**. Full canonical-order vector:
`[20,20,10,0,0,0,0,0,10,10,10,10,0,0,20,0,0,0,0]`.

The earlier claim that `Unit`, `Tiberium`, `Pod`, `Napalm` and `Squad` are
"disabled by default because they ship as `no`" is **wrong** and was corrected
2026-09-02: that token is over-water eligibility. `Unit` is one of the eight
positive-weight outcomes and drops normally on land; over water it is redirected
to `Money`. `Tiberium`, `Pod`, `Napalm` and `Squad` are unreachable because
their weight is `0`, exactly like `Cloak`, `Darkness`, `Explosion`, `ICBM`,
`Gas`, `IonStorm` and `Invulnerability`.

## Confidence

HIGH — all 19 name pointers resolved to ASCII literals, all 4 global
arrays bounded and sized from the ReadPowerups loop bound, every field
extractor (atoi / AnimType::Find_Index / yes-no strcmp / atof+%-scale)
confirmed in the decomp, CrateClass consumer xrefs match the
expected access patterns (weight-sum, over-water gate, parameter-load).

## Cross-refs

- RulesClass dispatcher step 32 (`FUN_00668BF0 @ +0x…`).
- `CRATE_SYSTEM_GHIDRA_REPORT.md` — pre-existing document on crate
  pickup behaviour. Cross-link added in Task 14 consolidation.
- `ini/rulesmd.ini` lines 30345–30366 — stock `[Powerups]` section.
