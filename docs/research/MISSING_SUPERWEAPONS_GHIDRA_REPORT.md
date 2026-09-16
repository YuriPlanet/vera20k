# Missing Superweapon Dispatch Handlers — Ghidra Report + Rust Handoff

**Scope:** the FIVE superweapon `kind`s that currently hit the "not yet implemented" arm in
`src/sim/world/world_commands.rs` (~L1324): **MultiMissile (Nuke), ChronoSphere, ChronoWarp,
PsychicDominator, SpyPlane**. The 6 working handlers (IronCurtain, LightningStorm, ParaDrop,
AmerParaDrop, GeneticConverter, PsychicReveal, ForceShield) live under `src/sim/superweapon/`
and define the shape this doc's handoffs mirror.

**Authority order:** binary → Ghidra → docs → ini. Every address/offset/formula cites its
source. Claims verified live in Ghidra THIS session are marked **[V-this-session]**; claims
carried from an existing verified doc are marked **[V-doc:<name>]**; inferences are **[INF]**.

**Status:** RESEARCH ONLY. No `src/` file was edited. Companion deep-dive docs already exist and
are extended, not redone:
`NUKE_SUPERWEAPON_GHIDRA_REPORT.md`, [CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md),
[PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PSYCHIC_DOMINATOR_SUPERWEAPON_GHIDRA_REPORT.md), `SUPERCLASS_SYSTEM_GHIDRA_REPORT.md`,
[SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md), [SPY_SATELLITE_REVEAL_RADAR_PIXEL_PIPELINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SPY_SATELLITE_REVEAL_RADAR_PIXEL_PIPELINE_GHIDRA_REPORT.md),
[AIRCRAFTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/AIRCRAFTCLASS_GHIDRA_REPORT.md), [REVEAL_Z_SHIFT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/REVEAL_Z_SHIFT_GHIDRA_REPORT.md).

---

## 0. Shared dispatch facts (all five)

### 0.1 `SuperClass::Launch` = `0x006CC390`
Verified this session: `get_function_callers(0x0065eab0)` → `SuperClass__Launch @ 006cc390`
**[V-this-session]**. (The `0x006CC200` in [CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md) is a
stale/approx address; the real entry and all case bodies are inside `0x006CC390`.) Dispatch is a
`switch` on `*(int*)(SuperWeaponTypeClass + 0xB4)` (the Type enum). Enum ordering
(`SUPERWEAPON_TYPE_CLASS_GHIDRA_REPORT.md §3`, string table `0x008425C0`):

```
0 MultiMissile  1 IronCurtain  2 LightningStorm  3 ChronoSphere  4 ChronoWarp
5 ParaDrop  6 AmerParaDrop  7 PsychicDominator  8 SpyPlane  9 GeneticConverter
10 ForceShield  11 PsychicReveal
```

### 0.2 No auto-fire inside the SW tick
`SuperClass__AI_Charging @ 0x006CC080` **[V-this-session]** only advances the charge timer, sets
the ready flags (`+0x6D/+0x6E/+0x6F`, `+0x74 = frame`), and plays the "ready" EVA (`VoxClass__PlayEVA`);
the `switch(Type)` arms are all empty `break`s. It does **not** call `Launch`. Every SW (SpyPlane
included) fires only through `SuperClass::Launch`, which is reached from a player/AI command. This
matches the Rust design: dispatch belongs in `world_commands.rs`'s `LaunchSuperWeapon` handler.
(Auto-fire *target selection* for SpyPlane is an AI/house concern — see §4.7 + YELLOW.)

### 0.3 Rust dispatch/reset contract already in place
`world_commands.rs` validates `inst.is_active && inst.is_ready`, resolves `kind`/`recharge`, calls
the handler, and on `true` calls `inst.reset_after_fire(recharge, tick)`. New handlers plug into
the same `match kind { … }` (`world_commands.rs:1269-1328`). **Exception:** ChronoSphere first-click
must NOT reset/recharge (see §2).

### 0.4 RNG streams
The sim uses `sim.superweapon_rng()` for SW-local randomness (lightning bolt scatter). Verified RNG
consumption in the 5:
- Nuke: warhead `Detonate` screen-shake `Random::Range` (render-only) + projectile `Vertical=yes`
  path (no scatter for GiantNuke). **[V-doc:NUKE §9]**
- ChronoSphere/ChronoWarp: **no RNG** — geometry is deterministic. **[V-doc:CHRONOSPHERE §3,§11]**
- PsychicDominator: **no RNG** in mind-control iteration (deterministic cell walk). **[V-doc:PD §4]**
- SpyPlane: `Mission_SpyPlane` draws `Random__RandomRanged(0,2)` on the mission-timer eval each
  scheduling pass (standard mission-cadence jitter). **[V-this-session, decompile 0x00417300]**

---

## 1. MultiMissile (Nuke) — `kind == MultiMissile`, case 0

### 1.1 Targeting model
Single target cell (one click). INI `[NukeSpecial] Action=Nuke`, `WeaponType=NukeCarrier`,
`AuxBuilding` (NukeSilo) drives the two-phase door path. `ini/rulesmd.ini:30803-30817`.
Case 0 in `SuperClass::Launch` splits on readiness bytes `+0x6D/+0x6E/+0x6F`
(`NUKE_SUPERWEAPON_GHIDRA_REPORT.md §2`): **Path B** (silo present, door not yet open) finds the
`AuxBuilding`, opens its door anim, stores target at `HouseClass+0x5784`, stores SW type at
`building+0x5F8`; **Path A** (all flags set) actually spawns the carrier missile. In stock YR the
silo door delay is the observable "missile rises after a beat" behaviour.

### 1.2 Effect + magnitudes (the full chain — [V-doc:NUKE §1,§5,§6])
Two-projectile chain, both `Vertical=yes`:
1. Carrier `GiantNukeUp` (from `[NukeCarrier]`, `Speed=100`) flies up to `DetonationAltitude=20000`;
   its `Warhead=NukeMaker` (`WarheadTypeClass+0x176`) triggers `FUN_0046b310` (SpawnDownwardNuke).
2. `FUN_0046b310` looks up `[NukePayload]` weapon, spawns `GiantNukeDown` aimed at the ORIGINAL
   target cell: `Damage=600`, `Speed=10`, `RadLevel=500`, `Warhead=NUKE`. `DetonationAltitude=30000`.
3. On the down-nuke's arrival `BulletClass::AI` name-checks the warhead == `"NUKE"` (`0x0081AF98`) →
   special path: screen flash `FUN_0053ab70` (hardcoded **30-frame** white flash, no INI key),
   radar event, `NUKEBALL` anim at impact, then `FUN_004251f0` (NukeGroundZero) applies
   `Apply_area_damage(cell, Rules.NukeWarhead=[Nuke], …)` — the ACTUAL blast + radiation come from
   `[SpecialWeapons] NukeWarhead=Nuke` (`RulesClass+0xF8C`), NOT the payload's own warhead.
Radiation: `RadLevel=500` on `[NukePayload]` creates a `RadSiteClass` at ground zero
(`WarheadTypeClass::Detonate` pre-chain, [V-doc:NUKE §9]).

### 1.3 Timing / recharge
`RechargeTime=10` (min) → 10×900 = **9000 frames** (`superweapon_type.rs` conversion). `IsPowered=true`
→ charge suspends on low power. Silo door delay is a per-building anim delay before Path A fires.

### 1.4 Cues
- `RechargeVoice=00-I154` (ready EVA). `EVA_NuclearMissileLaunched` on fire.
  ([EVA_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/EVA_SYSTEM_GHIDRA_REPORT.md) superweapon table.)
- Launch sound + `NukeTakeOff=NUKETO` anim at the silo (`RulesClass+0x98`, `BuildingClass::CreateFireAnim @ 0x0043B5E0`).
- Screen flash (30 frames) + screen shake from `[Nuke]` warhead `ShakeX/Yhi/lo`.

### 1.5 Gating
Target may be anywhere on the map (incl. shroud — nukes are not LOS-gated). `super_weapons` game
option guard already in `world_commands.rs`. If `AuxBuilding` (NukeSilo) is dead the SW is revoked
by the grant scan.

### 1.6 INI keys (stock, cited)
```
[NukeSpecial]  RechargeTime=10  Type=MultiMissile  WeaponType=NukeCarrier  SidebarImage=NukeIcon  ; :30803
[SpecialWeapons] NukeWarhead=Nuke  NukeDown=NukeDown  NukeProjectile=NukeUp                       ; :584-586
[NukeCarrier] Projectile=GiantNukeUp Speed=100 Warhead=NukeMaker
[NukePayload] Damage=600 Speed=10 RadLevel=500 Warhead=NUKE Projectile=GiantNukeDown
[General] NukeTakeOff=NUKETO
```

---

## 2. ChronoSphere (case 3) + ChronoWarp (case 4) — `kind == ChronoSphere / ChronoWarp`

**These two are one gameplay superweapon with a two-click flow.** `[ChronoSphereSpecial] PreClick=yes`
(`:30928`) + hidden `[ChronoWarpSpecial] PostClick=yes PreDependent=ChronoSphere` (`:30935-30948`).

### 2.1 Targeting model (TWO clicks — app work required, §6)
- **Click 1 → ChronoSphere (case 3):** stores SOURCE cell at `SuperClass+0x62`, sets cursor state
  `DAT_008809a0 = 4` (await-destination), starts the sphere building anim, and **returns without
  firing/recharging**. `CHRONOSPHERE_SUPERWEAPON_GHIDRA_REPORT.md §2`.
- **Click 2 → ChronoWarp (case 4):** reads stored source + clicked destination and performs the warp,
  then clears cursor state (`DAT_008809a0 = -1`) and recharges. §3.

### 2.2 Warp effect + magnitudes ([V-doc:CHRONOSPHERE §3,§11])
Iterates a **fixed 3×3 grid** (9 offsets at `g_CellSpreadOffsets 0x00B0C038`) around BOTH source and
destination; each source cell maps to the corresponding destination cell; per-unit sub-cell offset
is preserved. **No unit-count cap.** Per occupant:
- Must be on-map (`Flags & 0x04`) and not cloaked (vtable+0x54).
- `Chronoshiftable=no` (`TypeClass+0xD97`) AND `+0xCD4==0` → **killed instantly** with full HP damage
  via `Rules->C4Warhead` (`Rules+0xFA8`).
- Chronoshiftable → create `TeleportLocomotion` (CLSID `{4A582747-…}` @ `0x007E9A90`), piggyback the
  existing locomotor, set `ChronoInTransit`, write dest coords (`+0x288/0x28C/0x290`), set
  `PendingWarpPhase=3`, store `ChronoSourceHouse (+0x42C)` for kill credit.
- **Destination validity** (`PostWarpValidation 0x007187A0`, §10): warp onto **water** (non-amphibious,
  not infantry) → unit **dies**, credited to sphere owner; warp onto **impassable/occupied** → full
  C4 damage (occupants at dest are crushed). Bridge Z handled at both endpoints.
- Docked chrono-miner special case is skipped (`skipKill`). Chrono **miner** self-teleport is a
  SEPARATE system already ported — do not conflate (`feedback_chrono_*`).

### 2.3 Timing / recharge
ChronoSphere `RechargeTime=7` → **6300 frames**; `IsPowered=true`. ChronoWarp `RechargeTime=1`,
`IsPowered=false` — but the two share ONE charge in gamemd (second click consumes the charged sphere).
Post-warp per-unit "chrono lock" = `Rules->ChronoDelay=60` frames (`Rules+0xBEC`, `:221`) during
which the unit can't move and shimmers (`BeingWarped`).

### 2.4 Cues
`ChronoBlast=CHRONOFD` at source, `ChronoBlastDest=CHRONOTG` at dest (`:546-547`, `Rules+0x32C/+0x328`).
Radar events at both cells. `EVA_ChronosphereActivated`. Per-unit `ChronoInSound/ChronoOutSound`
sparkle + `LetsDoTheTimeWarpIn/OutAgain` sounds (`:738-739`).

### 2.5 Gating
Both clicks are cell picks; the warp affects all warpable units in the 3×3 source grid regardless of
ownership (you can warp enemy units — including into water to kill them). No shroud gate on target.

### 2.6 INI keys (cited)
```
[ChronoSphereSpecial] RechargeTime=7 Type=ChronoSphere PreClick=yes Range=1.4 LineMultiplier=3 ; :30916
[ChronoWarpSpecial]   RechargeTime=1 Type=ChronoWarp PostClick=yes PreDependent=ChronoSphere    ; :30935
[General] ChronoDelay=60 ChronoBlast=CHRONOFD ChronoBlastDest=CHRONOTG                          ; :221,546,547
per-type: Chronoshiftable=yes|no (TypeClass+0xD97)
```

---

## 3. PsychicDominator — `kind == PsychicDominator`, case 7

### 3.1 Targeting model
Single target cell. `[PsychicDominatorSpecial] Action=PsychicDominator Range=1.4` (`:30982-30996`).
Case 7 calls `PsychicDominator::Start (0x0053AE50)` + radar event + EVA + sound.

### 3.2 Effect + magnitudes — 5-phase state machine ([V-doc:PD §4])
`Start` creates `DominatorFirstAnim=PDFXCLD` (giant head) at target, sets `PD_State=1`, starts the
red ambient-lighting shift (`FUN_0053c280`). Per-tick `Process (0x0053AF40)` advances 1→2→3→4→5→0.
The **effect fires ONCE** at state 2→3 when the first anim reaches `DominatorFireAtPercentage=20%`
of its frames, via `PsychicDominator::Fire (0x0053B080)`:
- **Area damage:** `Apply_area_damage(0, Rules.DominatorWarhead=[DominatorWH], 1, PD_DamageOwnerHouse)`.
  `[DominatorWH] CellSpread=7`, `Verses` deals 0% to most armor, so units survive to be captured;
  buildings (filtered out of MC) take the damage. `DominatorDamage=1000` (`Rules+0x30C`, `:538`).
- **Mass mind control:** walks all cells within `DominatorCaptureRange` (stock **1**, capped ≤10,
  `:539`) using the CellSpread offset table. For each qualifying object:
  - filters: `WhatAmI()!=6` (skip buildings), `ImmuneToPsionics==0` (`TypeClass+0xD35`), not
    Iron-Curtained (vtable+0x160), `BalloonHover==0` (`TypeClass+0xD6A`), not in-limbo (vtable+0x54).
  - frees any existing CaptureManager, then `SetOwner(PD_OwnerHouse, 1)` = **PERMANENT** transfer
    (not range-broken MC), sets `IsPermanentlyMindControlled (+0x2C4)=1`, attaches
    `PermaControlledAnimationType=MINDANIMR` (red ring) at `MindControlRingOffset`.
- Second anim `DominatorSecondAnim=PDFXLOC` (ground ring) spawns at fire time.

### 3.3 Timing / recharge
`RechargeTime=10` → **9000 frames**, `IsPowered=true`. Effect timing is anim-frame-driven
(fire at 20% of PDFXCLD; state 3→4 at <11 frames left; 4→5 at <2; 5→0 when ambient fully restored).

### 3.4 Cues
`EVA_PsychicDominatorActivated`, `PsychicDominatorActivateSound` (`Rules+0x24C`), radar event, red
ambient lighting ramp+fade (shared controller `0x0053C280`, priority LS > Nuke > PD > normal).

### 3.5 Gating
Target cell anywhere. Capture radius is stock 1 cell (tight). Buildings never mind-controlled.

### 3.6 INI keys (cited)
```
[PsychicDominatorSpecial] RechargeTime=10 Type=PsychicDominator Range=1.4                    ; :30982
[General] DominatorWarhead=DominatorWH DominatorDamage=1000 DominatorCaptureRange=1
          DominatorFirstAnim=PDFXCLD DominatorSecondAnim=PDFXLOC DominatorFireAtPercentage=20 ; :537-542
[DominatorWH] CellSpread=7 Verses=0%,0%,0%,0%,0%,0%,100%,100%,6%,0%,0%                         ; :27568
[CombatDamage] ControlledAnimationType=MINDANIM PermaControlledAnimationType=MINDANIMR
```

---

## 4. SpyPlane — `kind == SpyPlane`, case 8  (HIGH FREQUENCY)

Every Soviet player owns `[NARADR]` (Soviet Radar, `SuperWeapon=SpyPlaneSpecial`,
`ini/rulesmd.ini:12601,12630`), so this cameo appears in nearly every Soviet match. **[V-this-session]**

### 4.1 Targeting model
Single target cell (`[SpyPlaneSpecial] Action=SpyPlane`, `:30999-31013`). Case 8 shares the ParaDrop
dispatch block: gets country/side (`FUN_0041caa0`), resolves the target cell (`0x005657a0`, guarded
against the off-map sentinel `0xABDC50`), then **spawns spy-plane aircraft via `FUN_0065EAB0`**
(distinct from paradrop's `FUN_0065E660`). **[V-this-session, xref 0x006cd6d3 → 0x0065eab0]**

### 4.2 `FUN_0065EAB0` (spy-plane spawner) — decompiled this session
Signature `(house param_1=ECX, aircraftTypeIdx param_2=EDX, count param_3, targetCell param_4/5, param_6)`.
Loop body per aircraft:
- `local_14 = g_AircraftTypeClass_Array[param_2]` — aircraft type by index.
- factory-create the aircraft owned by the house (`vtable+0x8C`), set byte `+0x3D4 = 1`.
- `vtable+0x1E8 (targetCell,0)` = assign NavCom/mission target; optional `vtable+0x480`/`+0x3C8`.
- `vtable+0xD8 (&edgeCell,0)` = Unlimbo/place at edge; on failure `vtable+0x20(1)` deletes it.
- `vtable+0x1EC ()` = commit mission. Returns count spawned.
Call site at `0x006cd6d3` pushes `count=1`, target cell, house = `SuperClass+0x2C`. The aircraft
type index and the per-side count/guard come from side-dependent Rules arrays around
`Rules+0xC4C/0xC68` (same family the ParaDrop block reads). **Exact aircraft-type array offset is
INF — see YELLOW.** Stock resolves to `[SPYP]` (Soviet Spy Plane) for the Soviet side.

### 4.3 The aircraft: `[SPYP]` (`ini/rulesmd.ini:11323-11361`)
```
Strength=600 Armor=light Speed=15 ROT=2 Ammo=100 Sight=0  Landable=no MoveToShroud=yes
Primary=SpyCameraWeapon  Spawned=yes  Selectable=no  FlyBy=true  ImmuneToPsionics=yes
CanPassiveAquire=no CanRetaliate=no  Locomotor={4A582746-…}(fly)  MovementZone=Fly
MoveSound=SpyPlaneMoveLoop  CrashingSound=SpyPlaneDie  DeathWeapon=BlimpBomb
```
It has `Sight=0`, so the reveal is NOT from unit sight — it comes from firing the camera.

### 4.4 The reveal: `[SpyCameraWeapon]` (`ini/rulesmd.ini:23193-23199`)
```
Damage=6   ; "range of shroud to reveal"  → REVEAL RADIUS = 6 cells
Range=20   ; "howfar away to start revealing" → weapon range
Projectile=InvisibleHigh  Warhead=DummyWarhead  Report=SpyPlaneSnapshot  Burst=1
```
`AircraftClass::Fire_At` calls `MapClass::RevealAroundCell (0x005678e0)` at `0x00416557/0x00416595`
(`REVEAL_Z_SHIFT_GHIDRA_REPORT.md §5`) — i.e. each camera shot reveals a **radius-6** disc around the
aimed cell for the owning house. As the plane overflies, this produces a **moving reveal along the
approach path to the target**, not a single static reveal.

### 4.5 Mission — `AircraftClass::Mission_SpyPlane (0x00417300)` [V-this-session]
State machine on `param_1[0x2F]`: 0 INIT (find attack cell from NavCom `[0x169]`), 1 SET_COURSE,
2 OVERFLY (fires the camera when in range; range check `< 0x100` leptons/`>0xFF` gating), 3/4
egress + land-cell check, then despawn. `owner IsCloaked (+0x430) = 1` — the plane is invisible.
Mission-timer eval draws `Random__RandomRanged(0,2)` (cadence jitter — **determinism-relevant**).
Camera sound cadence: `SpyPlaneCameraFrames=16` (`:736`), sound `SpyPlaneCamera=SpyPlaneSnapshot`
(`:735`). `FlyBy=true` → no slow-down over target.

### 4.6 Timing / recharge
`RechargeTime=4` → **3600 frames** (~4 min in-game; user observes ~3 min — see YELLOW on cadence),
`IsPowered=false` (charges even at low power), `ShowTimer=no`, `FlashSidebarTabFrames=120`.

### 4.7 Gating / auto-fire
`SuperClass::AI_Charging` has no auto-launch (§0.2). Auto-fire "at any Soviet player" is
AI/house target-selection that calls `Launch` with a chosen cell — the DISPATCH handler is what's
missing and is what this doc scopes. Auto-target selection is AI (`feedback_no_ai_yet`, deferred);
the handler must simply reveal + fly given a target cell.

### 4.8 INI keys (cited)
```
[SpyPlaneSpecial] RechargeTime=4 Type=SpyPlane IsPowered=false ShowTimer=no FlashSidebarTabFrames=120 ; :30999
[NARADR] SuperWeapon=SpyPlaneSpecial                                                                   ; :12630
[SPYP] Primary=SpyCameraWeapon Speed=15 Sight=0 FlyBy=true Ammo=100                                    ; :11323
[SpyCameraWeapon] Damage=6 Range=20 Report=SpyPlaneSnapshot                                            ; :23193
[AudioVisual] SpyPlaneCamera=SpyPlaneSnapshot SpyPlaneCameraFrames=16                                  ; :735-736
```

---

## 5. IMPLEMENTATION HANDOFF (per SW)

All new modules go under `src/sim/superweapon/`, mirror the 6 existing handlers'
`launch(sim, rules, owner, target_rx, target_ry) -> bool` signature (ParaDrop also takes
`path_grid`), push a `SimSoundEvent`, log, and let `world_commands.rs` do the reset/recharge.
Wire each into the `match kind` at `world_commands.rs:1269`.

### 5.1 Nuke (`src/sim/superweapon/nuke.rs`)
- **Sim state:** add `Simulation.pending_nukes: Vec<NukeInFlight { owner, rx, ry, phase, timer }>`
  (mirrors the up/down projectile chain) OR reuse the bullet/effect pipeline if a projectile system
  exists. Simplest faithful model: on `launch`, if the granting building has `AuxBuilding`, run the
  silo door delay before the up-phase; else start immediately.
- **Effect:** at ground-zero detonation apply AoE via the `[SpecialWeapons] NukeWarhead=Nuke` warhead
  through the existing `apply_aoe_damage` (see `lightning_storm.rs` for the AoE call shape),
  `Damage` from `[NukePayload] Damage=600`, plus a radiation site (`RadLevel=500`) if a rad system
  exists (else defer rad, note it). Emit `NUKEBALL` anim + `SimSoundEvent` + a 30-frame screen-flash
  request to the app layer (render-only).
- **Recharge:** handler returns true only when the missile is actually launched (Path A). For the
  silo-door path, either fire synchronously (approximation) or model the delay (preferred).
- **Tests:** `nuke_applies_nukewarhead_aoe_at_target`; `nuke_ground_zero_damages_3x3_by_cellspread`;
  `nuke_uses_specialweapons_nukewarhead_not_payload_warhead`.
- **Blockers:** projectile flight + rad-site systems may not exist yet → acceptable first cut is
  instantaneous ground-zero AoE + anim/flash, with the two-projectile visual deferred (note it).

### 5.2 ChronoSphere + ChronoWarp (`src/sim/superweapon/chrono_warp.rs`)
- **Two handlers, ONE shared charge.** `SuperWeaponKind::ChronoSphere` handler: store
  `inst.pending_chrono_source = Some((rx,ry))` on the instance (add field to `SuperWeaponInstance`),
  spawn the source anim (`ChronoBlast=CHRONOFD`), **return a sentinel that suppresses reset/recharge**
  (see wiring note). `SuperWeaponKind::ChronoWarp` handler: read the stored source, warp the 3×3 grid
  to the clicked destination, spawn `ChronoBlastDest=CHRONOTG` at dest, clear the pending source, and
  return true (so it recharges).
- **Warp logic:** iterate the 3×3 source grid; for each entity (any owner): if `Chronoshiftable=no`
  kill with C4 warhead; else move it to `dest_cell + (pos - source_cell_center)` preserving sub-cell,
  apply `ChronoDelay=60`-frame movement lock (reuse the existing `invulnerability`/timer pattern or a
  new `chrono_lock` flag on the entity), and destination validity: water (non-amphibious/non-infantry)
  → kill; occupied/impassable → C4 damage to occupants. Bridge-Z at both endpoints (reuse
  `bridge_adjusted_impact_z`).
- **Wiring change:** `world_commands.rs` currently resets on any `true`. Add a 3-state return or a
  `kind`-check: ChronoSphere first-click must NOT `reset_after_fire`. Cleanest: make the ChronoSphere
  arm set the pending source and `return true` but special-case `if kind != ChronoSphere` around the
  reset block, OR return an enum `{ Fired, Armed, Failed }`.
- **Tests:** `chronosphere_stores_source_and_does_not_recharge`;
  `chronowarp_moves_chronoshiftable_3x3_preserving_subcell`;
  `chronowarp_kills_non_chronoshiftable_with_c4`;
  `chronowarp_onto_water_kills_non_amphibious`.
- **Blockers:** needs the app-layer two-click flow (§6) and a per-entity chrono-lock; both are new.

### 5.3 PsychicDominator (`src/sim/superweapon/psychic_dominator.rs`)
- **Sim state:** optional `Simulation.psychic_dominator: Option<PdState>` for the 5-phase anim timing;
  a first cut can apply the effect immediately at launch (single-shot) and just spawn the anims,
  deferring the exact 20%-of-frames delay (note it).
- **Effect:** AoE damage via `[General] DominatorWarhead=DominatorWH` (`apply_aoe_damage`,
  `DominatorDamage=1000`), then mind-control all entities in `DominatorCaptureRange=1` cells that pass
  the filters (skip buildings; `ImmuneToPsionics==0`; not iron-curtained; not balloon-hover). Mind
  control = permanent owner change to `owner` + set a `permanently_mind_controlled` flag + attach
  `MINDANIMR` ring anim. Reuse whatever owner-transfer path the existing mind-control/`genetic_converter`
  spawn uses; if no MC system exists, this is the blocker.
- **Cues:** `EVA_PsychicDominatorActivated`, radar event, PDFXCLD/PDFXLOC anims, red-ambient request to
  app (render-only).
- **Tests:** `pd_captures_units_in_range_permanently`; `pd_skips_buildings_and_immune_units`;
  `pd_applies_dominatorwh_damage`.
- **Blockers:** requires a unit ownership-transfer / mind-control primitive. Verify one exists
  (`CaptureManager` analog) before implementing; if absent, that primitive is the real gap.

### 5.4 SpyPlane (`src/sim/superweapon/spy_plane.rs`) — do this first (frequency)
- **Model:** spawn a `[SPYP]` aircraft at the owner's edge (reuse `paradrop.rs`'s edge-cell +
  `spawn_object_at_height` + cruise-altitude setup), give it a new `AircraftMission::SpyPlaneOverfly {
  target_rx, target_ry, camera_timer }`, fly it toward the target, and each `SpyPlaneCameraFrames=16`
  ticks reveal a **radius-6** disc (`vision::reveal_radius`, exactly the `psychic_reveal.rs` call)
  around the plane's current cell for `owner`, emitting the `SpyPlaneSnapshot` sound. After the plane
  passes the target, fly it off-map and despawn.
- **Determinism:** if you model the mission-timer jitter, draw `superweapon_rng().next_range_u32(3)`
  (0..=2) to match `Random__RandomRanged(0,2)`; otherwise omit RNG entirely and document the drift.
- **Simplest acceptable first cut:** if the aircraft/mission pipeline is too heavy, reveal a radius-6
  disc at the target cell once (like `psychic_reveal` but radius 6) + spawn the plane sprite; note the
  missing moving-reveal-along-path as drift. Prefer the moving model since it's the observable behaviour.
- **Tests:** `spyplane_reveals_radius6_around_target_for_owner`;
  `spyplane_spawns_spyp_and_despawns_after_overfly`; `spyplane_recharges_3600_frames`.
- **Blockers:** the moving reveal needs a per-tick aircraft mission hook (air-movement already exists
  via `air_movement`); reveal radius = `SpyCameraWeapon.Damage` (6), NOT a `Sight` value.

### 5.5 Dispatch wiring (`src/sim/world/world_commands.rs:1324`)
Replace the `other => log::warn!(…)` arm with the five new `match` arms calling the handlers above.
Keep the `success → reset_after_fire` block, but exempt `ChronoSphere` (first click arms, does not
recharge). No change needed to the `is_active && is_ready` gate.

---

## 6. App-layer targeting-flow changes (ChronoSphere/ChronoWarp only)

Today `TargetingMode::SuperWeapon(String)` (`app_types.rs:255`) is single-click:
`launch_super_weapon_at_cursor` (`app_commands.rs:345`) schedules one `Command::LaunchSuperWeapon`
at the click cell and clears the mode. Nuke, PD, SpyPlane fit this unchanged.

**ChronoSphere/ChronoWarp need a two-click flow.** Recommended (matches gamemd's SuperClass+0x62
source-store + cursor-state-4):
1. Add `TargetingMode::SuperWeaponTwoClick { section: String, source: Option<(u16,u16)> }` (or a
   `source` field on the existing variant).
2. First tactical click with `source==None`: schedule `LaunchSuperWeapon { sw_type_id=ChronoSphereSpecial,
   target=click }`, set `source=Some(click)`, keep the mode armed, and switch the cursor to the
   "chrono destination" cursor. Do NOT clear `targeting_mode`.
3. Second tactical click: schedule `LaunchSuperWeapon { sw_type_id=ChronoWarpSpecial, target=click }`,
   clear the mode. The sim's ChronoSphere handler stored the source on the instance (§5.2), so the
   `Command` shape (single cell) is sufficient — no need to widen the command with a source cell.
4. Right-click / Esc while `source==Some` cancels: the sim must also drop the pending source (add a
   `CancelChrono` command or clear on right-click) so a half-armed sphere doesn't strand.

Grant note: verify GACSPH (Chronosphere building) grants BOTH `ChronoSphereSpecial` and
`ChronoWarpSpecial` so the ChronoWarp instance exists to be launched; the second click's
`is_active && is_ready` gate is checked against the ChronoWarp instance. In gamemd they share a charge;
in Rust the simplest parity is: ChronoWarp inherits readiness from the ChronoSphere first-click having
armed a pending source (gate ChronoWarp on `pending_chrono_source.is_some()` instead of its own timer).

---

## 7. Acceptance summary

| SW | Targeting | Core observable | Recharge (frames) | Hardest blocker |
|----|-----------|-----------------|-------------------|-----------------|
| Nuke | 1 click | ground-zero `[Nuke]` AoE + radiation + 30-frame flash | 9000 | projectile chain + rad site |
| ChronoSphere | click 1/2 | store source, arm dest cursor (no recharge) | shared | two-click app flow |
| ChronoWarp | click 2/2 | warp 3×3 chronoshiftable, kill rest (C4), water-death | shared w/ sphere | per-entity chrono-lock |
| PsychicDominator | 1 click | permanent mass MC in radius 1 + `DominatorWH` AoE | 9000 | unit ownership-transfer primitive |
| SpyPlane | 1 click | SPYP overflies, radius-6 moving reveal, snapshot sound | 3600 | per-tick aircraft mission + reveal |

---

## YELLOW — unverified / needs one more pass

- **SpyPlane aircraft-type array offset.** Case 8 reads a side-dependent Rules array (family of
  `Rules+0xC4C/0xC68`) for the aircraft-type index and per-side count/guard; I confirmed the CALL args
  (`count=1`, target cell, house) but did NOT isolate the exact offset that yields the `[SPYP]` type
  index. **[INF]** Stock behaviour resolves to SPYP for Soviet; verify the offset before hardcoding.
- **SpyPlane count.** The outer loop bound (`Rules+0xC4C`, guarded by `==Rules+0xC68`) governs how many
  planes spawn; stock is effectively 1 but I did not read the stock value from the parsed array.
- **SpyPlane recharge vs user observation.** INI `RechargeTime=4` → 3600 frames (~4 min at 15 fps game
  time); user reports ~3 min. Difference may be frame-rate/EVA timing or an AI auto-fire cadence, not
  the charge timer. Confirm the observed cadence source before "fixing" the recharge.
- **SpyPlane auto-fire trigger.** No auto-launch in `SuperClass::AI_Charging` (verified). The
  "auto-fires for any Soviet player" behaviour must originate in AI/house target-selection that calls
  `Launch`; that path was not decompiled this session (AI is deferred: `feedback_no_ai_yet`).
- **Nuke silo two-phase timing.** Path B door-open delay (`building+0x5F8` pending type, door anim) is
  documented but the exact frame delay before Path A fires was not measured; first-cut may fire
  synchronously.
- **PsychicDominator anim-frame timing.** The 20%-of-first-anim fire trigger depends on PDFXCLD frame
  count from art(md).ini, not yet enumerated here.
- **`SuperClass::Launch` case-body addresses** (e.g. exact case-4 ChronoWarp sub-address) are carried
  from `CHRONOSPHERE_…` doc as approximations; the dispatch entry `0x006CC390` is verified this session.
