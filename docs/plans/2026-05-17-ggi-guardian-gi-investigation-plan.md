# Guardian GI (GGI) — Investigation Plan

> **Status: EXECUTED 2026-05-17.** Output: [`GGI_GHIDRA_REPORT.md`](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GGI_GHIDRA_REPORT.md)
> — 9 sections, ~55KB, covers parse path, deploy state machine, fire/AA gate,
> crush gate, BFRT/IFV routing, weapon/projectile/warhead readers, damage
> formula with verified rounding mode, missile homing flight curve. Includes
> a parity-trap finding: `ProneDamage` is dead data in YR — see §9.1 of the
> report.
>
> **For Claude:** This plan scopes a `/re-investigate` pass on the GGI unit.
> Already executed; do not re-run. Future targeted follow-ups (the remaining
> §7 / §9 open items in the report) can use specific function lists, not
> this plan.

**Topic:** Guardian GI (`[GGI]`) — Allied secondary infantry. Two-weapon
deployer: walks with M60 (anti-infantry), deploys stationary to fire
`MissileLauncher` (`AAHeatSeeker2`, AA+AG capable) at 8-cell range. Distinct
unit from the basic GI (`E1`, M60+Para sandbag), already covered in
[GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md).

**Scope Size:** Medium — ~24 functions in inventory, ~52 GGI-specific rules
keys + ~20 art/sequence keys, 4 weapons (`M60` + `M60E` + `MissileLauncher` +
`MissileLauncherE`), 2 warheads (`SA`, `GUARDWH`), 2 projectiles
(`InvisibleLow`, `AAHeatSeeker2`).

**Est. Effort:** ~6–9 hours of `/re-investigate` work (~6 FULL-depth functions
× 20 min, ~12 MEDIUM × 8 min, ~6 LIGHT × 3 min, plus synthesis).

**Prior Research:**
- [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md) — complete E1 dossier, **reusable** for shared
  InfantryClass infrastructure (AI loop, fire pipeline, panic/fear, sub-cell,
  mind control, render, voice, crush). Do NOT redo this surface.
- [IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md) — `OpenTransportWeapon=1`
  path verified; GGI fires missile from BFRT confirmed. **Reusable.**
- [WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md), [WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md),
  [BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md) — generic. **Reusable.**
- [FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md) — generic infantry firing. **Reusable.**
- `2026-05-16 disparity-scan-gi-unit.md` — already flags
  `DeployedCrushable=no` as missing in Rust; informs GGI scope.

**Expected Output:** research document at
[docs/research/GGI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GGI_GHIDRA_REPORT.md)

**Next Pipeline Step:** `/brainstorm` GGI-specific Rust integration → then
`/write-plan` for implementation. Most plumbing (deploy state, secondary
weapon dispatch, AA targeting bits, rocket projectile) already exists in
Rust — the brainstorm scope is wiring + AA-target preference + the
GGI-specific gaps surfaced.

---

## 1. Goal

When this investigation finishes, the report must let an implementer answer
every observable-behavior question about GGI:

- Which weapon does GGI fire in which state (walking M60, deployed
  MissileLauncher, garrisoned UCPara-equivalent if any, IFV missile)?
- What is the exact deploy/undeploy state machine (frame counts from
  sequence, locks on movement during deploy/undeploy, can-fire window)?
- How does GGI's secondary weapon select air vs ground targets — does
  deployed GGI prefer aircraft over infantry? Does it auto-fire on air?
- What is `AAHeatSeeker2`'s exact flight behavior (homing ROT, arming time,
  speed) — does it match the existing rocket renderer's output?
- What are `GUARDWH`'s exact Verses values, `ProneDamage`, `CellSpread`,
  `PercentAtMax` damage falloff? `SA` same questions for the M60 path.
- How does `DeployedCrushable=no` actually block crush in gamemd? What's
  the call site?
- What are the elite-tier deltas (M60E damage, MissileLauncherE damage/ROF/Speed)?
- How does `IFVMode=16` resolve — what does the IFV turret look like, what
  weapon does the BFRT use? (Note: `IFVMode=16` here is the BattleFortress
  slot index, not the IFV gunner ID — disambiguate during execution.)
- Are there GGI-specific voice triggers, custom panic behavior, or
  deploy-fear gates that differ from E1?

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps re: GGI |
|--------|-------|------------|---------------------|
| [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md) | E1 dossier: parse, AI loop, fire, damage, XP, panic, sub-cell, garrison, IFV, weapon validators, locomotor, render, voice, cursor | HIGH | Title misleadingly says "Guardian GI / E1" but covers E1 only. No GGI specifics; no MissileLauncher; no AAHeatSeeker2; no GUARDWH; no IFVMode=16 BFRT path. |
| [IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md) | `OpenTransportWeapon` and IFV gunner system | HIGH | Confirms GGI's `OpenTransportWeapon=1` semantics. Does NOT cover BFRT-side IFVMode lookup table. |
| [FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md) | Infantry `Fire_At_Target` + animation sync | HIGH | Doesn't distinguish primary vs secondary path; does not cover deployed-fire weapon select. |
| [WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md) | WeaponTypeClass offsets | HIGH | Generic. Reusable for MissileLauncher fields. |
| [WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md) | WarheadTypeClass offsets | HIGH | Generic. Reusable for SA, GUARDWH layout. |
| [BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md) | Projectile creation pipeline | HIGH | Generic. Reusable for AAHeatSeeker2 init. |
| [TECHNOCLASS_COMBAT_WEAPON_SYSTEMS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_COMBAT_WEAPON_SYSTEMS_REPORT.md) | `SelectWeaponAgainst`, weapon-by-target | HIGH | Generic. Reusable for GGI primary/secondary decision. |
| `2026-05-16 disparity-scan-gi-unit.md` (in-repo) | E1 + GGI parity audit vs Rust | n/a | Already flags `DeployedCrushable=no` Rust gap for GGI. Reference for what's broken in Rust today. |

**Conflicts between reports:** None found. The only inaccuracy is the title
of [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md) claiming to cover Guardian GI; the body is E1 only.

## 3. Function Inventory

Grouped by execution phase. Phase 1 = entry/parse + core fire path. Phase 2 =
weapon/warhead/projectile depth + deploy state machine. Phase 3 = callers,
edges, AA targeting preference, BFRT IFVMode=16 resolution.

| # | Phase | Address | Current Name | Scope Reason | Depth | TS-Risk |
|---|-------|---------|--------------|--------------|-------|---------|
| 1 | 1 | `0x005240a0` | `InfantryTypeClass__ReadINI` | `[GGI]` parse entry; verify which GGI-specific keys land where (`Deployer`, `Secondary`, `ElitePrimary`, `EliteSecondary`, voices, `DeployedCrushable`). Already partially covered by E1 doc — extract diffs only. | MEDIUM | Low |
| 2 | 1 | `0x00714xxx` (TechnoTypeClass__ReadINI) | unnamed | Reads `IFVMode` at `0x714787`, `OpenTransportWeapon` at `0x714e5c`, `DeployFire` at `0x7147ef`, `DeployFireWeapon` at `0x7147d5`, `DeployingAnim` at `0x714715`, `IsSelectableCombatant` at `0x715757`, `SecondaryFireFLH` at `0x715e1e`, `EliteSecondaryFireFLH` at `0x715f20`, `FireAngle` at `0x714b85`, `DeployTime` at `0x714b5d`. Extract struct offsets for each. | MEDIUM | Low |
| 3 | 1 | `0x005236a0` | `InfantryTypeClass__Constructor` | Defaults for Deployer flag, Crushable flags, OpenTransportWeapon (default -1) | LIGHT | Low |
| 4 | 1 | `0x00523980` | `InfantryTypeClass__Constructor` (alt) | Confirm whether this is copy ctor or alt path | LIGHT | Low |
| 5 | 1 | `0x0051bab0` | `InfantryClass__AI` | Per-tick driver. Already in E1 doc — only extract the deploy/secondary-fire branches and what they read from type. | LIGHT | Low |
| 6 | 1 | `0x0051d6f0` | `InfantryClass__Do_Action` | Doing-state entry for sequence groups; `Deploy`/`Deployed`/`DeployedFire`/`DeployedIdle`/`Undeploy` are seq groups 0x1B–0x1F. Extract transition rules. | FULL | Low |
| 7 | 1 | `0x00520ae0` | `InfantryClass__DoType_Sequencer` | Picks frame range from sequence per Doing state — extract Deploy-state frame resolution and the FireUp anchor frame for deployed fire (artmd `FireUp=2`). | FULL | Low |
| 8 | 2 | `0x005206b0` | `InfantryClass__Fire_At_Target` | Weapon select per deploy state — verify deployed GGI dispatches Secondary unconditionally vs based on target. | FULL | Low |
| 9 | 2 | `0x0051df70` | `InfantryClass__Fire_At_Override` | Forced fire path — check if it's the deployed-fire entry or a TS bombard-mission holdover. | MEDIUM | Medium — possible TS legacy |
| 10 | 2 | `0x005218e0` | `FUN_005218e0` (unnamed, ~0x7d bytes) | Infantry weapon-select driver (calls `SelectWeaponAgainst`). Decompile fully — label-after-verify candidate. | FULL | Low |
| 11 | 2 | `0x006f3330` | `TechnoClass__SelectWeaponAgainst` | Primary-vs-secondary selector. Confirm deploy-state branch and AA-target branch — does it prefer Secondary when both targets exist but only Secondary is AA? | FULL | Low |
| 12 | 2 | `0x0070e140` | `TechnoClass__GetWeapon` | Weapon-by-index lookup. Verify infantry path (caller table showed only Building uses it directly — confirm infantry inlines). | LIGHT | Low |
| 13 | 2 | `0x006f3970` | `TechnoClass__GetWeaponRange` | Range gating per weapon idx — confirms deployed GGI has range 8 vs walking 4. | MEDIUM | Low |
| 14 | 2 | `0x006f77b0` | `TechnoClass__CanFireAt` | Eligibility filter — does it block air targets when Secondary can fire AA? Verses gate? | MEDIUM | Low |
| 15 | 2 | `0x0051cdb0` | `InfantryClass__UpdateIdleAction` | Idle anim while deployed — picks `DeployedIdle` vs `Deployed` | LIGHT | Low |
| 16 | 2 | `0x0051cba0` | `InfantryClass__IdleDispatch` | Idle Doing dispatch — confirm deployed idle holds | LIGHT | Low |
| 17 | 2 | `0x00521b20` | `InfantryClass__Clear_Doing_Action` | Reset path; how does undeploy clear back to normal locomotion? | MEDIUM | Low |
| 18 | 2 | `0x0070fec0` | `TechnoClass__IsDeploying` | Deploy-state predicate. Confirm InfantryClass uses it vs has its own. | LIGHT | Medium — also serves MCV |
| 19 | 3 | `WeaponTypeClass__ReadINI` (address TBD) | unknown | Parse `MissileLauncher` keys: `AA`, `AG` not seen as plain strings — confirm whether they're projectile keys (on `AAHeatSeeker2`) vs weapon keys. Extract `MinimumRange`, `Burst`, `Speed`. | MEDIUM | Low |
| 20 | 3 | `BulletTypeClass__ReadINI` (address TBD) | unknown | Parse `AAHeatSeeker2`: `AA=yes`, `AG=yes`, `Arm=2`, `ROT=60`, homing logic | MEDIUM | Low |
| 21 | 3 | `WarheadTypeClass__ReadINI` (address TBD) | unknown | Parse `GUARDWH` and `SA`: `Verses`, `ProneDamage`, `CellSpread`, `PercentAtMax`, `InfDeath`, `AnimList`. Extract exact damage tables. | MEDIUM | Low |
| 22 | 3 | `0x0051e3b0` | `InfantryClass__What_Action_OnObject` | Cursor/action picker. Determines deploy-on-target cursor behavior and whether deploy is offered as an action when an air target is visible. | MEDIUM | Low |
| 23 | 3 | `0x0051f800` | `InfantryClass__What_Action_OnCell` | Cell action — deploy-on-cell entry, verify "deploy here" cursor logic | MEDIUM | Low |
| 24 | 3 | (TBD — search `DeployedCrushable` xref) | unknown | The crush eligibility check on InfantryClass — find where `Crushable` vs `DeployedCrushable` branch lives. Confirm the gate so Rust can wire it correctly. | FULL | Low |

**Out of scope (will NOT be decompiled):**
- `WarheadTypeClass__Detonate` `0x004690b0` — only relevant to AbductorWarhead-style TS deploy paths, not GGI.
- `TechnoClass__OnDeployBegin` `0x0070fc90`, `OnUndeployComplete` `0x0070fbe0` — vehicle (MCV) paths, not infantry deploy.
- Per E1 doc §6 — all generic infantry behavior (panic, fear, sub-cell, mind control, render, voice routing). Extract GGI deltas only.

**Phase 1 checkpoint rule:** after #1–#7 are decompiled, pause and produce a
skeleton report (parse path + deploy state machine identified). If the
"primary fire vs deployed fire" branch turns out to live somewhere other than
where Agent D pointed (i.e., not in `Fire_At_Target` but in
`SelectWeaponAgainst`'s deploy check), revise Phase 2/3 before continuing.

## 4. Detail Checklist

The executor must extract every item below. Items are anchored where the
scoping scan already located them.

### Magic numbers / constants

- Sequence group constants 0x1B–0x1F (27–31) — `Deploy`, `Deployed`,
  `DeployedFire`, `DeployedIdle`, `Undeploy` — confirm exact mapping from
  `Do_Action` / `DoType_Sequencer`.
- `DeployTime` field offset (read at `0x714b5d`).
- `FireAngle` field offset (read at `0x714b85`) — does GGI have a unique angle?
- M60 `Damage=15`, `Range=4`, `ROF=20`; M60E `Damage=25`.
- MissileLauncher `Damage=40`, `Range=8`, `ROF=40`, `Burst=1`, `Speed=30`,
  `MinimumRange=1`; MissileLauncherE `Damage=50`, `ROF=20`, `Speed=40`.
- AAHeatSeeker2 `Arm=2`, `ROT=60`.
- SA `ProneDamage=70%`, `InfDeath=1`.
- GUARDWH `ProneDamage=50%`, `InfDeath=3`, `CellSpread=0.5`, `PercentAtMax=0.5`.
- Cost 400, Strength 100, Soylent 150, Points 10, TechLevel 2.

### Bit flags and masks

- `Deployer`, `DeployFire`, `IsSimpleDeployer` — are these separate bits or
  one bitfield? Decompile reader for each.
- `Crushable` vs `DeployedCrushable` — likely two separate bools; confirm.
- `OpenTransportWeapon` default `-1` vs GGI's explicit `1` — int field.
- `ImmuneToVeins=yes` bit — verify in TechnoTypeClass offset.
- `AA=yes` / `AG=yes` on AAHeatSeeker2 — bits on BulletTypeClass.

### State machine

- `Doing` enum values for: walking-with-Primary, prone-with-Primary,
  Deploying, Deployed (idle), DeployedFire, DeployedIdle, Undeploying,
  Panic. Note this overlaps with E1's three "low-silhouette" states from
  E1 doc — extract whether GGI uses the same enum slots.
- Transition table: which inputs (player command, fire start/end, idle
  timeout) move between which states?

### Verses tables

```
SA      = 100, 80, 80, 50, 25, 25, 75, 50, 25, 100, 100
GUARDWH = 20,  20, 20, 100, 50, 100, 10, 10, 10, 100, 100
```

Verify by decompiling `WarheadTypeClass__ReadINI` and dumping the Verses
array offset. Confirm armor-type ordering matches what we use today.

### INI keys to verify (from Agent B)

All keys listed in Section 5 below. Each must trace to a `Read*` call inside
`InfantryTypeClass__ReadINI` or `TechnoTypeClass__ReadINI`. If a key is in
the INI but not read by either path, flag it as parsed-but-unused.

### Struct offsets to extract

- TechnoTypeClass offsets: `IFVMode`, `OpenTransportWeapon`, `DeployFire`,
  `DeployFireWeapon`, `DeployingAnim`, `IsSelectableCombatant`,
  `SecondaryFireFLH` (vec3), `EliteSecondaryFireFLH` (vec3), `FireAngle`,
  `DeployTime`.
- InfantryTypeClass offsets: `Deployer`, `Crushable`, `DeployedCrushable`,
  `Pip`, secondary weapon ptr.
- WeaponTypeClass offsets relevant to MissileLauncher (already in
  [WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md) — verify only).
- WarheadTypeClass `Verses` array offset (extract the byte/int width).
- BulletTypeClass `AA`/`AG`/`ROT`/`Arm` offsets.

**Reminder on `param_1` arithmetic** — per CLAUDE.md, BulletTypeClass uses
`int` so offsets are direct bytes; AnimTypeClass uses `int *` so any
`param_1[N]` indexing means `N × 4` byte offset. Note the type before
copying offsets.

### Clamps, rounding, off-by-ones

- AAHeatSeeker2 ROT=60 — what's the per-tick max turn in fixed-point?
- MissileLauncher MinimumRange=1 — verify whether 1 cell or 1 lepton
  (256 leptons/cell).
- ProneDamage % application — round-half-up, truncate, or round-to-even?
- CellSpread=0.5 — damage falloff curve.

### Edge cases to test

- Deploying while taking damage — does it cancel deploy?
- Issuing a move command while deployed — does it auto-undeploy and start
  moving, or reject the command?
- Air target enters range while walking GGI is attacking infantry — does it
  switch targets, switch weapons, or both?
- Air target enters range while deployed GGI has no current target — does
  it auto-acquire?
- GGI inside BFRT (`IFVMode=16`): which BFRT weapon slot fires; does it
  use BFRT turret rotation; what happens when only one GGI is inside vs
  multiple (BFRT round-robin?).
- GGI inside garrison building — does it use UCPara-equivalent? (Note: GGI
  has no `UCPara`-style entry in INI; check whether OccupyWeapon defaults
  apply or if it can even garrison.)
- `Occupier=no` confirmed for GGI in INI — does it actually block garrison
  via the cursor/action check? Verify.
- `IsSelectableCombatant=yes` — verify it's added to select-all-combat
  group.
- Mind controlled GGI: does deploy still work under enemy control?

### Timing / ordering

- Within `advance_tick`, GGI fits in:
  - commands (deploy command queued) → ground movement (locked during
    deploy/undeploy) → turrets+combat (deployed fire dispatch) →
    retaliation (deployed retaliation) → scatter/anims
- Confirm whether the deploy state machine advances on the AI step or the
  animation step.

### TS-legacy flags

See Section 7.

### Vtable dispatches

- `InfantryClass__Do_Action` `0x0051d6f0` — no callers via direct xref.
  Likely virtual dispatch from `InfantryClass__AI`. Resolve via vtable
  read.
- `InfantryClass__Fire_At_Override` `0x0051df70` — same; resolve through
  TechnoClass vtable.

## 5. INI Keys in Scope

### `[GGI]` rules section (rulesmd.ini 3863–3913)

| Key | Default | Suspected Purpose | Parsed in Rust today? |
|-----|---------|-------------------|------------------------|
| `UIName=Name:GuardianGI` | — | UI string key | Yes (generic) |
| `Name=Guardian GI` | — | Display name | Yes (generic) |
| `Category=Soldier` | — | AI task force class | Yes |
| `Cost=400` | — | Build cost | Yes |
| `Soylent=150` | — | Refund value | Yes |
| `Points=10` | — | Score on kill | Yes (generic) |
| `TechLevel=2` | -1 | Min tech to build | Yes |
| `Primary=M60` | — | Walking weapon | Yes |
| `Secondary=MissileLauncher` | — | Deployed weapon | Yes (parsed) |
| `ElitePrimary=M60E` | — | Veteran walking weapon | Yes (parsed) |
| `EliteSecondary=MissileLauncherE` | — | Veteran deployed weapon | Yes (parsed) |
| `Strength=100` | — | HP | Yes |
| `Armor=none` | — | Damage class | Yes |
| `Speed=3` | — | Walk speed | Yes |
| `Sight=6` | — | LOS range | Yes |
| `Locomotor={GUID}` | — | COM locomotor (Foot) | Yes |
| `MovementZone=Infantry` | — | Pathfind terrain class | Yes |
| `PhysicalSize=1` | — | Cargo slot size | Yes |
| `Crushable=yes` | — | Crushed by vehicles | Yes |
| `DeployedCrushable=no` | — | Crushed when deployed | **NO — Rust gap (G4 in disparity scan)** |
| `Deployer=yes` | — | Can deploy | Yes (`deploy.rs`) |
| `DeployFire=yes` | — | Stationary fire from deploy | Yes (parsed) |
| `IFVMode=16` | -1 | BFRT slot index | **Partial — see Open Q** |
| `OpenTransportWeapon=1` | -1 | Transport weapon override | Yes |
| `Owner=British,...` | — | Faction list | Yes |
| `AllowedToStartInMultiplayer=no` | — | Skirmish start eligibility | Yes |
| `Prerequisite=GAPILE` | — | Build dep | Yes |
| `Occupier=no` | yes | Can garrison | Yes (parsed) |
| `ImmuneToVeins=yes` | — | Vein creature immunity | Yes (parsed) |
| `ImmuneToPsionics=no` | — | Mind control immunity | Yes (parsed) |
| `Bombable=yes` | — | Paradrop bomb target | Yes (parsed) |
| `IsSelectableCombatant=yes` | — | Select-all-combat group | **Unverified** |
| `Pip=white` | — | Transport pip color | Yes |
| `VoiceSelect=GuardianGISelect` | — | Click voice | Yes |
| `VoiceMove=GuardianGIMove` | — | Move voice | Yes |
| `VoiceAttack=GuardianGIAttackCommand` | — | Attack voice | Yes |
| `VoiceFeedback=GuardianGIFear` | — | Damage voice | Yes |
| `VoiceSpecialAttack=GuardianGIMove` | — | Special attack voice | Yes (parsed) |
| `DieSound=GuardianGIDie` | — | Death sound | Yes |
| `DeploySound=GuardianGIDeploy` | — | Deploy SFX | **NO — Rust gap** |
| `UndeploySound=GIUndeploy` | — | Undeploy SFX | **NO — Rust gap** |
| `CrushSound=InfantrySquish` | — | Crush SFX | Yes |
| `VeteranAbilities=...` | — | Vet bonus list | Yes |
| `EliteAbilities=...` | — | Elite bonus list | Yes |
| `ThreatPosed=10` | — | AI threat weight | Yes |
| `PixelSelectionBracketDelta=-6` | 0 | Bracket Y offset | Yes |

### `[GGI]` art section (artmd.ini 291–299)

| Key | Value | Purpose | Parsed in Rust? |
|-----|-------|---------|------------------|
| `Cameo=GDGIICON` | — | Sidebar icon | Yes |
| `AltCameo=GDGIUICO` | — | Fog-of-war cameo | Yes |
| `Sequence=GuardianGISequence` | — | Animation set | Yes |
| `Crawls=yes` | — | Prone-while-crawling | Yes |
| `Remapable=yes` | — | House color remap | Yes |
| `FireUp=2` | — | Bullet-spawn frame within FireUp sequence (this is a TechnoTypeClass key on the art section per ModEnc — verify) | **Unverified** |
| `PrimaryFireFLH=80,0,105` | — | Muzzle origin (M60) | Yes |
| `SecondaryFireFLH=80,0,90` | — | Muzzle origin (MissileLauncher) | Yes |

### `[GuardianGISequence]` (artmd.ini 14166–14191)

20 sequence keys: `Ready`, `Guard`, `Walk`, `Crawl`, `Idle1`, `Idle2`,
`Panic`, `FireUp`, `FireProne`, `Deploy`, `Deployed`, `DeployedFire`,
`Undeploy`, `Down`, `Up`, `Prone`, `Die1..Die5`, `Cheer`, `Paradrop`.
Already enumerated in Rust `SequenceKind` per Agent C.

### Weapons / warheads / projectiles

- `[M60]`, `[M60E]` — primary + elite primary.
- `[MissileLauncher]`, `[MissileLauncherE]` — secondary + elite secondary.
- `[SA]` warhead — M60 path. `[GUARDWH]` warhead — Missile path.
- `[InvisibleLow]` projectile — M60. `[AAHeatSeeker2]` projectile — Missile.

All keys for these sections enumerated in Agent B output; the
research doc must verify each key's struct offset and any default
fallback.

## 6. Caller & Integration Map

Callers from Agent D's xref hop:

| Caller | Calls Into | When | Decompile? |
|--------|------------|------|------------|
| `Logic_AI` top-level | `InfantryClass__AI 0x0051bab0` | Every tick | LIGHT (already in E1 doc) |
| `FUN_005218e0` | `TechnoClass__SelectWeaponAgainst 0x006f3330` | Infantry weapon-pick | YES — Phase 2 #10 |
| `FUN_00746cd0` | `TechnoClass__SelectWeaponAgainst 0x006f3330` | Unit weapon-pick | NO (out of GGI scope) |
| `BuildingClass__GetWeapon 0x004526f0` | `TechnoClass__GetWeapon 0x0070e140` | Building fire | NO |
| (TBD) | `InfantryClass__Do_Action 0x0051d6f0` via vtable | Doing transition | Resolve via vtable read in Phase 1 |
| (TBD) | `InfantryClass__Fire_At_Override 0x0051df70` via vtable | Force-fire | Resolve via vtable; flag TS suspicion |

**Rust integration today** — where GGI's outputs will be consumed:

- `src/sim/deploy.rs` — `DeployPhase` state machine already exists; the
  report must give the executor frame-accurate timing so Phase 2 brainstorm
  can wire art.ini Deploy/Undeploy frame counts into it (currently hardcoded
  `DEPLOY_DEFAULT_TICKS=55`).
- `src/sim/combat/combat_weapon.rs` — `select_weapon()` already gates
  Primary/Secondary by AA/AG. The report must confirm whether gamemd uses
  the **same** rule (projectile-flag-driven) or whether the deploy state
  itself forces Secondary unconditionally regardless of target type.
- `src/sim/movement/rocket_movement.rs` — already implements ballistic
  flight. The report must confirm AAHeatSeeker2's homing ROT/Arm match
  what Rust simulates.
- `src/rules/warhead_type.rs` — Verses parsed but **not applied in damage
  calc** (Agent C flagged). Report must specify exact application formula
  so brainstorm/plan can wire it.
- `src/rules/object_type.rs` — `DeployedCrushable` parsing must be added.
- `src/sim/passenger.rs` — `IFVMode=16` (BFRT) routing not wired; report
  must specify how BFRT's per-slot weapon table is keyed.

**Callers NOT investigated and why:**
- All `Logic_AI` top-level callers — out of GGI scope; covered by main loop docs.
- All vehicle-side deploy paths (`OnDeployBegin`, `OnUndeployComplete`,
  `WarheadTypeClass__Detonate` deploy hook) — vehicle/MCV/TS warhead, not
  infantry.

## 7. TS-Legacy Risk Register

Watch carefully — the points below all came up during scoping and need
explicit verification during execution.

- **`InfantryClass__Fire_At_Override` `0x0051df70`** — name suggests a
  forced fire path. May be the deployed-fire entry, but the "Override"
  naming is suspicious. Could be a TS bombard-mission/airstrike holdover.
  **Verify reachability in YR** before treating it as deployed-fire.
- **`TechnoClass__IsDeploying` `0x0070fec0`** — shared between MCV (vehicle
  deploy to MCV→ConYard transformation) and infantry deploy. Must confirm
  the InfantryClass path doesn't accidentally pick up MCV-specific branches.
- **`TechnoClass__OnDeployBegin/OnUndeployComplete`** — vehicle-only paths.
  Explicitly NOT in scope. If decompilation ever lands in them, back out
  immediately.
- **`WarheadTypeClass__Detonate` `0x004690b0`** — calls
  `TechnoClass__PerformDeploy` for AbductorWarhead-style TS deploy. Not
  reached in stock YR by GGI. Skip.
- **AbductorWarhead / chrono / unit-transform deploy paths** — none of
  these apply to GGI. Don't follow them.
- **`AntiAircraft`, `AGFireCoord`, `AAFireCoord`, `AA=` strings** — Agent
  D could not find them as plain strings. They may be composed at parse
  time, or this may be an indication that AA flagging happens at the
  *projectile* level (`AAHeatSeeker2.AA=yes`) not the weapon level. Phase
  3 #19/#20 must resolve which.
- **Sequence groups 0x20–0x22** (`Idle1`, `Idle2`, `Panic`) — `Panic` is
  the E1 doc Phase 2 entry. Confirm GGI uses the same panic state machine
  with the same fear thresholds, or note divergence.

## 8. Current Rust Implementation Surface

From Agent C — what already exists; what's missing per file:

| File | Status | Notes |
|------|--------|-------|
| [src/sim/deploy.rs](../../src/sim/deploy.rs) | Partial | `DeployPhase` enum implemented; hardcoded 55-tick deploy duration; no per-type frame resolution; no DeploySound trigger; no movement lock. |
| [src/sim/infantry.rs](../../src/sim/infantry.rs) | Partial | `is_deploy_locked()` and fear/prone logic; no deploy-specific fear gates. |
| [src/rules/object_type.rs](../../src/rules/object_type.rs) | Mostly complete | Parses `DeployFire`, `DeployFireWeapon`, `DeploySound`, `Secondary`, `IFVMode`, `OccupyWeapon`. Missing: `DeployedCrushable`. |
| [src/rules/infantry_sequence.rs](../../src/rules/infantry_sequence.rs) | Complete | All deploy sequence variants present in `SequenceKind`. |
| [src/sim/animation.rs](../../src/sim/animation.rs) | Mostly | Deploy variants present; `DeployedFire` auto-trigger relies on `attack_target.is_some()` rather than deploy-state gate. |
| [src/sim/movement/rocket_movement.rs](../../src/sim/movement/rocket_movement.rs) | Complete | Ballistic + homing flight, deterministic SimFixed math. |
| [src/rules/projectile_type.rs](../../src/rules/projectile_type.rs) | Complete | AA/AG flags, ROT, Arm, homing — all 37 INI keys parsed. |
| [src/sim/combat/combat_targeting.rs](../../src/sim/combat/combat_targeting.rs) | Partial | AA/AG projectile gating; no air-only preference; no auto-acquire when only Secondary is AA-capable. |
| [src/sim/combat/combat_weapon.rs](../../src/sim/combat/combat_weapon.rs) | Mostly | `select_weapon()`, `select_garrison_weapon()`, IFV override; suppressed-weapon retaliation not enforced. |
| [src/rules/warhead_type.rs](../../src/rules/warhead_type.rs) | Parsed-not-applied | `Verses` array parsed, **not applied in damage calc**. Critical gap. |
| [src/sim/passenger.rs](../../src/sim/passenger.rs) | Partial | `PassengerCargo` + garrison round-robin; no BFRT IFVMode=16 slot routing. |

**Net Rust gaps surfaced for GGI** (the brainstorm output should target these):

1. `DeployedCrushable=no` not parsed or checked.
2. `DeploySound` / `UndeploySound` parsed but not triggered on phase change.
3. Deploy state does not lock movement at the command layer.
4. Verses % parsed but not multiplied into damage.
5. AA target preference when both ground+air are in range and Secondary is the only AA-capable weapon.
6. BFRT `IFVMode=16` slot routing.
7. Deploy duration hardcoded to 55 ticks instead of reading from art sequence frame count.
8. Fear/panic gates not deploy-aware (E1 also has this gap, but worth confirming for GGI).
9. `IsSelectableCombatant` may not be enforced in select-all dispatch.

## 9. Deferred Open Questions

These the scoping pass couldn't resolve — Phase 1/2/3 must answer them:

1. Does `IFVMode=16` indicate weapon slot 16 in the BattleFortress weapon
   list (`Weapon1..17`), or does it map to the BFRT's IFV-style turret
   selector? GGI's IFVMode=16 vs IFV (`FV`) gunner IDs — what does the
   value space actually mean?
2. Is `FireUp=2` in the GGI art section the bullet-spawn frame (within the
   FireUp sequence) or a different field entirely? Verify which class reads
   it and what offset.
3. Does deployed GGI prefer air targets over ground when both are in range
   (because its only fire option is the Secondary which is AA-capable)? Or
   does it use generic target priority and only switch weapons after
   target choice?
4. Can a non-deployed GGI auto-acquire an air target — does the AI
   recognize "I need to deploy to shoot this aircraft" and auto-deploy?
5. Does `InfantryClass__Fire_At_Override` get called for deployed fire, or
   is the deployed-fire path the normal `Fire_At_Target` with a weapon
   index change?
6. Is `IsSimpleDeployer` `0x00845dfc` true for GGI? Does that flag change
   behavior vs `Deployer=yes` alone?
7. Where does the `DeployTime` field (read at `0x714b5d`) actually slow or
   gate the deploy state? Is it on TechnoTypeClass and applies to
   infantry, or vehicle-only?
8. Garrison: GGI has no `OccupyWeapon=`. With `Occupier=no` (?), can it
   even be garrisoned? Verify cursor/action path.
9. Are the elite weapon swaps (`M60`→`M60E`, `MissileLauncher`→`MissileLauncherE`)
   triggered at the Veteran tier or the Elite tier? Does promoting from
   Veteran to Elite re-swap mid-life?

## 10. Execution Strategy

**Recommended: batched subagents with phase checkpoints.**

- **Phase 1 batch** (5 subagents, parallel): functions #1, #2, #6, #7, #10.
  Synthesis pass produces the parse-path + deploy-state-machine skeleton.
  **Checkpoint before Phase 2.**
- **Phase 2 batch** (6 subagents, parallel): #3, #4, #5, #8, #9, #11.
  Plus #13, #14, #17 (sequential after #11 settles SelectWeaponAgainst's
  branches).
- **Phase 3 batch** (mix of MEDIUM/LIGHT): #12, #15, #16, #18, #19, #20,
  #21, #22, #23, #24. Caller resolutions for vtable dispatches go here.
- **Synthesis**: produce the final [GGI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GGI_GHIDRA_REPORT.md).

The plan is too large for a single `/re-investigate` session; the batching
above keeps each agent's scope to ≤3 functions and lets the synthesis pass
own the cross-cutting state-machine diagram and verses tables.

## 11. Success Criteria

The executed research document must:

- Answer every question in Section 1 with citations.
- Cover every function in Section 3 (or explicitly justify omission).
- Resolve every deferred question in Section 9 — or re-document it as
  still unresolved with a clear reason.
- For every finding, state **Active in YR: Yes / No / Conditional** with
  the gating condition named.
- For every HIGH-confidence claim, cite a Ghidra address.
- For every claim about a vtable dispatch, show the `read_memory` read
  that confirmed the binding (per the
  [[feedback_vtable_binding_verification]] memory rule).
- For RE function citations, use the 3-axis confidence (content, identity,
  binding) per [[feedback_research_confidence_axes]].
- Before declaring HIGH binding, must have run `get_function_callers` and
  caller traces per [[feedback_caller_trace_before_finding]].
- Filter every finding for TS-legacy reachability — no claim survives
  without "is this hot in YR skirmish?" answered.
- Cross-link to [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md) for every shared-with-E1 fact rather
  than restating; only document GGI deltas.

## Sources

- **Ghidra addresses sampled** (scoping only):
  `0x005240a0`, `0x00714xxx` (per-key addresses listed in §3),
  `0x005236a0`, `0x00523980`, `0x0051bab0`, `0x0051d6f0`, `0x00520ae0`,
  `0x00521b20`, `0x005206b0`, `0x0051df70`, `0x005218e0`, `0x006f3330`,
  `0x0070e140`, `0x006f3970`, `0x006f77b0`, `0x0051cdb0`, `0x0051cba0`,
  `0x0070fec0`, `0x0051e3b0`, `0x0051f800`, `0x004526f0`.
- **Docs searched:** [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md), [IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md),
  [FIRE_AT_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FIRE_AT_PIPELINE_GHIDRA_REPORT.md), [TECHNOCLASS_COMBAT_WEAPON_SYSTEMS_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_COMBAT_WEAPON_SYSTEMS_REPORT.md),
  [WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WEAPONTYPECLASS_FULL_STRUCT_LAYOUT.md), [WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WARHEADTYPECLASS_FULL_STRUCT_LAYOUT.md),
  [BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETCLASS_INIT_AND_FIRE_GHIDRA_REPORT.md), `2026-05-16-disparity-scan-gi-unit.md`.
- **INI files checked:** `ini/rulesmd.ini` `[GGI]` 3863, `[M60]` 22922,
  `[M60E]` 25281, `[MissileLauncher]` 22569, `[MissileLauncherE]` 25123,
  `[SA]` 26466, `[GUARDWH]` 26902, `[InvisibleLow]` 25385,
  `[AAHeatSeeker2]` 25678; `ini/artmd.ini` `[GGI]` 291,
  `[GuardianGISequence]` 14166, `[MGUN-*]` 16241.
- **Related plans:** none prior on GGI specifically.
