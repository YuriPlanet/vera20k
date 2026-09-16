# GI Unit Complete — Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass on the GI (Guardian GI / E1)
> infantry unit in Yuri's Revenge. Execute it by running `/re-investigate gi unit complete`
> with this plan loaded as context. The function inventory in §3 is grouped into three
> phases — produce a Phase 1 checkpoint summary before starting Phase 2.

**Topic:** Guardian GI / E1 — every observable behavior of the basic Allied infantry
unit in a YR skirmish, sufficient for indistinguishable parity with gamemd.exe.

**Scope Size:** Large — 47 entry points across 16 categories, ~50 INI keys (E1 + 6
weapon profiles + 4 warheads + 2 projectiles), heavy cross-system reach (combat,
garrison, IFV, sub-cell, veterancy, panic, render).

**Est. Effort:** ~12–16 hours of `/re-investigate` work
(≈ 6 × 30 min FULL + 18 × 10 min MEDIUM + 23 × 5 min LIGHT, plus synthesis).
Recommend executing Phase 1 in one session, then approval gate before Phase 2/3.

**Prior Research:** 17 reports touch GI mechanics; none cover GI-as-a-unit.
Strongest existing coverage: INFANTRYCLASS_GHIDRA_REPORT, INFANTRY_SUBCELL_POSITIONING,
FOOTCLASS_COMPLETE, GARRISON_SYSTEM, GARRISON_OCCUPANT_SYSTEM, VETERANCY_SYSTEM,
CHAOS_DRONE_BERSERK, CRUSH_SYSTEM, IFV_AND_OPEN_TOPPED_TRANSPORT, MIND_CONTROL_SYSTEM,
RECEIVE_DAMAGE, DAMAGE_MATH. The investigation **synthesizes** these into one GI
report; it does NOT re-cover ground already verified at HIGH confidence — only
GI-specific gaps and integration questions.

**Expected Output:** [docs/research/GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md)

**Next Pipeline Step:** `/brainstorm` for a GI implementation design (deploy-fire
state machine, panic/fear runtime, kill-count veterancy, mission handlers for
infantry are all currently missing in Rust), then `/write-plan`.

---

## 1. Goal

When this investigation finishes, the resulting report must answer:

> "What does the GI do, in a YR skirmish, at 99% parity — every weapon, every
> animation transition, every state, every INI key, every interaction with other
> systems — with addresses cited from gamemd.exe?"

Specifically the report must let an implementer answer without further research:

1. What stats and INI keys define a GI? (full E1 + weapon + warhead + art surface)
2. What is the deploy-fire state machine? (sequence transitions 0x1B–0x1E,
   trigger/abort conditions, weapon swap mechanics, blockers like bridges/water)
3. How does the GI's IFV passenger weapon dispatch work? (IFVMode=2 → CRM60)
4. How does garrison fire pick the GI's UCPara/UCElitePara weapon, with round-robin?
5. What is the prone vs deployed vs standing damage-taken multiplier?
6. How does veterancy promotion happen on this unit? (kill count, weapon swap)
7. How does sub-cell positioning + occupancy + walk-to-sub-cell work for GIs?
8. What spawn paths produce a free GI? (Cloning Vats, paradrop, survivors)
9. How does panic/fear interact with the GI's normal AI loop?
10. What does the renderer draw — selection bracket, pips, cameo, voice cues?

## 2. Prior Research Inventory

| Report | Confidence | GI Coverage | Gap |
|--------|------------|-------------|-----|
| [INFANTRYCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRYCLASS_GHIDRA_REPORT.md) | HIGH | Struct layout, DoType sequencer, fear, IsCrawling, Fire_At_Target | Doesn't cite a single unit — generic infantry only |
| [INFANTRY_SUBCELL_POSITIONING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRY_SUBCELL_POSITIONING.md) | HIGH | 3-per-cell, sub-cell 2/3/4, walk locomotor placement | Notes Rust bug `[0,3,4]` should be `[2,3,4]` |
| [FOOTCLASS_COMPLETE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_COMPLETE_GHIDRA_REPORT.md) | HIGH | Parent fields 0x520–0x6BF, NavCom, deploy flag 0x6AD | Parent class only; no GI specifics |
| [FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_MOVE_GHIDRA_REPORT.md) | HIGH | Mission_Move with infantry crawl-state branch | Full coverage; cross-link only |
| [FOOTCLASS_MISSION_ATTACK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/FOOTCLASS_MISSION_ATTACK_GHIDRA_REPORT.md) | HIGH | Mission_Attack with infantry panic-state branch | Full coverage; cross-link only |
| [GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) | HIGH | OccupyWeapon, EliteOccupyWeapon, round-robin, OccupyDamage/ROFMult | OK; need to confirm GI's UCPara math |
| [GARRISON_OCCUPANT_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_OCCUPANT_SYSTEM_GHIDRA_REPORT.md) | HIGH | Occupant DVec layout, AddOccupy/RemoveOccupy | OK |
| [VETERANCY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VETERANCY_SYSTEM_GHIDRA_REPORT.md) | HIGH (5 passes) | Kill XP formula, thresholds, attribution | OK; need to confirm GI's elite weapon swap timing |
| [IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/IFV_AND_OPEN_TOPPED_TRANSPORT_GHIDRA_REPORT.md) | HIGH | IFVMode dispatch, SetGunnerWeapon, turret index | OK; cross-link only |
| [CRUSH_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CRUSH_SYSTEM_GHIDRA_REPORT.md) | HIGH | Crushable, DeployedCrushable, deployed-state field 0x2A4 | Conflict: 0x2A4 byte identity (prone vs deployed?) |
| [RECEIVE_DAMAGE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RECEIVE_DAMAGE_GHIDRA_REPORT.md) | HIGH | TechnoClass::ReceiveDamage chain | Doesn't isolate InfantryClass override |
| [DAMAGE_MATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DAMAGE_MATH_GHIDRA_REPORT.md) | HIGH | Verses + ProneDamage application | `InfantryDamageMultiplier` not traced |
| [MIND_CONTROL_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MIND_CONTROL_SYSTEM_GHIDRA_REPORT.md) | HIGH | CaptureManager, ImmuneToPsionics, overload | OK; cross-link |
| [CHAOS_DRONE_BERSERK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CHAOS_DRONE_BERSERK_GHIDRA_REPORT.md) | HIGH | Berserk flag 0x298, Psychedelic warhead | Berserk vs fear interaction undocumented |
| [MAGNETRON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAGNETRON_SYSTEM_GHIDRA_REPORT.md) | HIGH | SizeWeight gate excludes infantry | OK |
| [MCV_DEPLOY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MCV_DEPLOY_GHIDRA_REPORT.md) | HIGH | Deploy state machine for vehicles | Different code path; reference only |
| `UNIT_MISSION_DEPLOY_BUILDING_GHIDRA_REPORT.md` | (not read) | Generic deploy mission | Reference only |

**Conflicts to resolve during /re-investigate:**

1. **Sequence 0x1B vs 0x1F** — INFANTRYCLASS report claims 0x1B sets IsProne=1 → 0x1C
   and 0x1F clears IsProne → 0x0. Verify exact semantics: which byte (0x68D
   `ShouldFireFromProne` vs 0x6DB `IsCrawling` vs 0x2A4)? Which is animation-trigger
   vs animation-playing?
2. **Byte 0x2A4** — CRUSH report says "deployed state at 0x2A4"; FOOTCLASS_COMPLETE
   doesn't list 0x2A4 explicitly. Either both prone-and-deployed share this byte
   (likely — it's a "low-profile" indicator) or one report is wrong.
3. **Round-robin garrison fire vs elite weapon selection** — does CurrentFireIdx
   advance per-shot (so an elite occupant only sometimes uses EliteOccupyWeapon
   on his turn) or per-occupant-death? Confirm in `BuildingClass__UpdateGarrisonFire`.
4. **Fear (0x6D4) vs berserk (0x298)** — both can override AI. Which wins if both
   set? The report on Chaos Drone says berserk clears mission to Hunt; does that
   suppress fear-driven prone?
5. **Sub-cell 0 vs 2/3/4** — Does sub-cell 0 ever appear on a placed infantry, or
   is 0 only the "uninitialized" sentinel? Rust uses `[0,3,4]` which the doc says
   is wrong, but the executor must confirm 2 is the correct first slot.

## 3. Function Inventory

47 functions, three phases. Phase 1 produces a usable skeleton report; checkpoint
before Phase 2.

### Phase 1 — Core (the GI in isolation)

| # | Addr | Name | Scope Reason | Depth | TS Risk |
|---|------|------|--------------|-------|---------|
| 1 | `0x005240a0` | `InfantryTypeClass__ReadINI` | **Primary INI surface** for infantry-specific keys (Crawls, Pip, Voice list). Decodes every key on [E1]. | FULL | low |
| 2 | `0x00712170` | `TechnoTypeClass__ReadINI` | Parent reader — owns DeployFire (0x6AC), DeployFireWeapon (0x6A8), IFVMode, Voice* | FULL | low |
| 3 | `0x00517a50` | `InfantryClass__Constructor` (A) | Spawn-path ctor; sets initial sub-cell, facing, vtable | MEDIUM | low |
| 4 | `0x00517cc0` | `InfantryClass__InitFromType` | Per-type init — sets prone-fire bools, Crawls, weapon defaults | MEDIUM | low |
| 5 | `0x0051bab0` | `InfantryClass__AI` | **Per-tick brain** — calls Mission_Capture, Fire_At_Target, DoType, FootClass::Locomotion_AI; branches on seq 0x1B–0x1E and vtable +0x1EC/+0x200 | FULL | low |
| 6 | `0x00520ae0` | `InfantryClass__DoType_Sequencer` | Sequence advancement — resolves 0x1B–0x1E transitions (deploy/undeploy/prone) | FULL | low |
| 7 | `0x0051d6f0` | `InfantryClass__Do_Action` | Per-sequence action dispatcher (which sub-frame fires the bullet, which plays a sound) | FULL | low |
| 8 | `0x005206b0` | `InfantryClass__Fire_At_Target` | Decides standing vs prone vs deploy fire path; weapon-slot selection for E1's M60 vs Para | FULL | low |
| 9 | `0x0051df70` | `InfantryClass__Fire_At_Override` | vtable Fire_At — preps sequence then calls TechnoClass::Fire_At | MEDIUM | low |
| 10 | `0x005227f0` | `InfantryClass__ReceiveDamage` | Infantry override — applies prone/deploy multiplier, picks gore animation, triggers panic | FULL | low |
| 11 | `0x00702d40` | `TechnoClass__RecordKill` | Kill registration → veterancy XP gain → promotion | MEDIUM | low |

**Phase 1 checkpoint deliverable:** A skeleton GI report that already covers
"what is a GI, how does it parse, when does it fire what weapon, how does it
take damage, how does it gain veterancy". After this, the user must approve
before Phase 2 spending — if the scope assumptions broke (e.g., #5 turned out
to be a thin wrapper around something else), revise the plan.

### Phase 2 — Depth (state machines, garrison, IFV, sub-cell)

| # | Addr | Name | Scope Reason | Depth | TS Risk |
|---|------|------|--------------|-------|---------|
| 12 | `0x0051cba0` | `InfantryClass__IdleDispatch` | Idle-state arms (random fidget) | LIGHT | low |
| 13 | `0x0051cdb0` | `InfantryClass__UpdateIdleAction` | Idle frame ticker | LIGHT | low |
| 14 | `0x00521b20` | `InfantryClass__Clear_Doing_Action` | Cancels current sequence on mission change | LIGHT | low |
| 15 | `0x00520f40` | `FootClass__Locomotion_AI` | Walk locomotor tick (sub-cell pursuit) | MEDIUM | low |
| 16 | `0x00521d80` | `InfantryClass__GetMovementSpeed` | Speed multiplier — prone, fear, terrain | MEDIUM | low |
| 17 | `0x0051d0d0` | `InfantryClass__Scatter` | Scatter on threat — refuses scatter while in seq 0x1B–0x1E | LIGHT | low |
| 18 | `0x0070dc70` | `TechnoClass__SetGunnerWeapon` | IFVMode dispatch — confirms GI's IFVMode=2 → host Weapon3=CRM60 | MEDIUM | low |
| 19 | `0x0043e7b0` | `BuildingClass__UpdateGarrisonFire` | **Garrison occupant fire** — per-tick UCPara/UCElitePara dispatch, round-robin index, multiplier application | FULL | low |
| 20 | `0x004525f0` | `BuildingClass__CanGarrison` | Eligibility filter | LIGHT | low |
| 21 | `0x00522910` | `BuildingClass__AddGarrisonOccupant` | Occupant insertion | MEDIUM | low |
| 22 | `0x004575b0` | `BuildingClass__EjectOccupants` | Eject-on-sell/IC/death | MEDIUM | low |
| 23 | `0x00481180` | `CellClass__PlaceInfantryInCell` | **3-per-cell allocator** — picks slot 2/3/4, marks occupancy bits | FULL | low |
| 24 | `0x0075c240` | `WalkLocomotionClass__FindSubCellDest` | Walk-into-cell sub-cell choice | FULL | low |
| 25 | `0x005217c0` | `InfantryClass__MarkCellOccupancy` | Cell stamp on entry | MEDIUM | low |
| 26 | `0x00521850` | `InfantryClass__UnmarkCellOccupancy` | Cell stamp clear on exit | MEDIUM | low |
| 27 | `0x0048e480` | `CellClass__InitSubCellOffsets` | Pixel-offset table init | MEDIUM | low |
| 28 | `0x00521c10` | `InfantryClass__Panic_SetFear300` | Forces fear=300 (warhead-driven panic) | MEDIUM | low |
| 29 | `0x00518c00` | `InfantryClass__SetFear` | Generic fear setter | LIGHT | low |
| 30 | `0x005200b0` | `InfantryClass__Fear_Decay_Handler` | Per-tick fear decay | MEDIUM | low |
| 31 | `0x00750010` | `VeterancyClass__IsElite` / `0x0074ff90` IsVeteran / `0x0074ffc0` IsNormalRookie | Tier checks gate elite weapon swap | LIGHT | low |
| 32 | `0x00750090` | `VeterancyStruct__SetVeteran` / `0x007500b0` SetElite | Promotion side-effects | MEDIUM | low |
| 33 | `0x0070e140` | `TechnoClass__GetWeapon` | Picks Primary/Secondary/Elite based on tier and target | MEDIUM | low |
| 34 | `0x006f3330` | `TechnoClass__SelectWeaponAgainst` | Primary-vs-Secondary chooser (M60 anti-inf vs Para anti-veh? confirm) | MEDIUM | low |
| 35 | `0x006f77b0` | `TechnoClass__CanFireAt` | Range / target validity | LIGHT | low |
| 36 | `0x006fc0b0` | `TechnoClass__GetFireError` | Fire blockers (cloak, EMP, range) | LIGHT | low |

### Phase 3 — Context & edges (spawn, MC, render, voice)

| # | Addr | Name | Scope Reason | Depth | TS Risk |
|---|------|------|--------------|-------|---------|
| 37 | `0x004157c0` | `AircraftClass__Mission_ParaDropOverfly` | Paradrop spawn — drops free GIs from Cloning Vats and AI paradrops | MEDIUM | low |
| 38 | `0x00415c60` | `AircraftClass__Drop_Payload` | Drop helper → calls PlaceInfantryInCell | MEDIUM | low |
| 39 | `0x0065ec30` | `ChronoSphere__WarpUnitsAtCell` | Chronosphere arrival placement | LIGHT | low |
| 40 | `0x00442d90` | `BuildingClass__SpawnSurvivors` | Confirm GI is/isn't in survivor pool | LIGHT | low |
| 41 | `0x00471d40` | `CaptureManagerClass__CaptureUnit` | Yuri-style mind control on GI (ImmuneToPsionics=no) | MEDIUM | low |
| 42 | `0x0053b080` | `PsychicDominator__MindControlArea` | Dominator AOE on GI | LIGHT | low |
| 43 | `0x004690b0` | `WarheadTypeClass__Detonate` | Locate Psychedelic / panic warhead branches that affect GI | MEDIUM | maybe — many TS branches |
| 44 | `0x00744720` | `UnitClass__OnEnterCell_Triggers` | **Crush-kill path** — vehicle drives over GI sub-cell → kill | FULL | low |
| 45 | `0x00522600` | `InfantryClass__IronCurtain` | IC-on-infantry kills the GI rather than protecting | LIGHT | low |
| 46 | `0x006f5190` | `TechnoClass__DrawExtras` | Selection bracket + pips + health on GI | MEDIUM | low |
| 47 | `0x005f65d0` | `TechnoClass__DrawVeterancyPips` | Vet pip rendering | LIGHT | low |
| 48 | `0x00637840` | `ObjectSelection__PlayVoice` | VoiceSelect/Move/Attack playback | LIGHT | low |
| 49 | `0x00752480` | `VoxClass__QueueVoice` | EVA queueing for "Unit lost" | LIGHT | low |
| 50 | `0x0051e3b0` | `InfantryClass__What_Action_OnObject` | Cursor decision on right-click target | MEDIUM | low |
| 51 | `0x0051f800` | `InfantryClass__What_Action_OnCell` | Cursor decision on right-click cell | MEDIUM | low |
| 52 | `0x004d97a0` | `FootClass__Evaluate_Target_Threat` | Retaliation scoring | MEDIUM | low |
| 53 | `0x00709820` | `TechnoClass__Retaliate_And_Scan` | Retaliation entry from ReceiveDamage | MEDIUM | low |

(Final count: 53 functions across 3 phases. Above the 50-fn split threshold by 3
— accepted because all 53 are tightly cohesive around one unit, and Phase 3 is
intentionally light cross-links to existing reports.)

## 4. Detail Checklist

The executor must extract each of these and cite the originating address.

**Stats / INI surface — from §1 funcs**
- Every key on [E1] in rulesmd.ini decoded to its struct offset
- DeployFireWeapon=1 default → Secondary slot (Para). Confirm at TechnoTypeClass+0x6A8 read site.
- IFVMode=2 → IFV's Weapon3 (CRM60). Confirm at SetGunnerWeapon read site.
- Voice list parser path (CCINIClass__ReadSoundList @ 0x525430) — list-vs-single key resolution

**Magic numbers**
- Sequence byte values 0x1B / 0x1C / 0x1D / 0x1E (and 0x1F if used) — what each maps to
- Fear thresholds (FearLevel 0x6D4): when does GI start crawling, when does it panic-flee
- Veterancy thresholds: 1.0 Veteran, 2.0 Elite (verified by VETERANCY_SYSTEM, just cite)
- Sub-cell quadrant table at `0x0089e9f0`
- ROF in frames: M60=20, Para=15, UCPara=15 (just cite from INI; report includes shot cadence)
- ProneDamage% (M60.warhead.SA = 70%, Para.warhead.SSA = 80%, UCPara.warhead.SSAB = 50%)
- DeployFireFLH = SecondaryFireFLH = 80,0,90 (artmd.ini)

**State machine — central artifact of the report**
- States: Standing, Walking, Prone-firing, Deployed-firing, Crawling, Panicking, Boarding,
  Garrisoned, Captured (MC), Dying. Diagram each transition with the function that drives it.
- Deploy: input trigger (D key / right-click-self?) → seq 0x1D → sets IsDeployed → seq 0x1E
  loop → fire uses DeployFireWeapon → undeploy on move command → exit transition
- Undeploy blockers: bridge, water, transport, MC'd, panicked — confirm at trigger site
- Crawl: Crawls=yes flag + receiving fire → IsCrawling=1 → speed ×Rules.CrawlsRate → seq 0x1B

**Bit flags at known offsets**
- 0x68D ShouldFireFromProne
- 0x6DB IsCrawling
- 0x6AD IsDeploying (FootClass)
- 0x2A4 — **conflict to resolve**: prone, deployed, or both
- 0x6AC DeployFire bool (TechnoType)
- 0x6A8 DeployFireWeapon int (TechnoType, default 1)
- 0xEAC DeployedCrushable (InfantryType)
- 0xEB4 Occupier (InfantryType)
- 0xEB5 Assaulter (InfantryType, irrelevant for GI but worth noting)
- 0x690 BerserkFriendly (TechnoType, irrelevant for GI)

**Struct offsets to extract** (cross-verify with FOOTCLASS_COMPLETE, INFANTRYCLASS reports — only NEW offsets)
- InfantryTypeClass full layout for E1-relevant fields
- WeaponTypeClass for M60/Para/UCPara/UCElitePara/M60E/ParaE/CRM60
- WarheadTypeClass for SA/SSA/SSAB
- BulletTypeClass for InvisibleLow/InvisibleHigh

**Clamps / rounding traps**
- ROF measured in frames vs actual frame ticks (standard 15fps lockstep)
- ProneDamage as fixed-point % (basis points 7000/8000/5000)
- Sight=5 → cell radius (verify integer cell, not lepton)

**Edge cases to test in the report**
- GI on bridge — can he deploy? does sub-cell allocation work on bridge cells?
- GI in IFV alone vs GI + GI + GI — does each contribute to gunner index? (No — only
  the first passenger; verify)
- GI in garrison + Iron Curtain on building — eject? frozen? immune?
- GI MC'd by Yuri Prime — does he still deploy? Does AI command him?
- GI hit by Magnetron — confirm he's exempt by SizeWeight gate
- GI runs over by Battle Fortress vs Apocalypse (OmniCrusher vs Crusher) vs Rhino (just Crusher)
- GI in Cloning Vats with full Cloning Vats production limit — what happens?
- Sub-cell collision: 3 GIs land same cell same frame
- GI fires from garrison while elite — does each shot use EliteOccupyWeapon, or only
  some shots (round-robin question)
- Panicked GI: can he fire? (FOOTCLASS_MISSION_ATTACK says player infantry in
  panic seq cancel the move; confirm he refuses fire)

**Timing / ordering in advance_tick**
- Where does panic decay tick fit in the Rust sim's tick order?
- Where does occupancy mark/unmark fit relative to movement step? (Critical for
  determinism — must mark before adjacent tile collision check)

**TS-legacy register** — see §7

**Vtable dispatches** to resolve concretely
- InfantryClass vtable +0x1D4, +0x1D8, +0x1EC, +0x200 — name them via RTTI table
- Vtable +0x1EC is `ActivateDeployFire`, +0x200 is `CanDeployFire` (per
  INFANTRYCLASS report — confirm)

## 5. INI Keys in Scope

**Section [E1] — rulesmd.ini lines 3713–3759**

| Key | Default | Notes / Currently Parsed |
|-----|---------|---------------------------|
| UIName, Name, Image, Category | — | Yes (object_type.rs) |
| Primary=M60, Secondary=Para | — | Yes |
| ElitePrimary=M60E, EliteSecondary=ParaE | — | Yes (elite swap) |
| Occupier=yes | false | Yes |
| OccupyWeapon=UCPara, EliteOccupyWeapon=UCElitePara | — | Yes |
| OpenTransportWeapon=1 | 0 | Yes |
| Prerequisite=GAPILE, TechLevel=1, Cost=200, Soylent=100 | — | Yes |
| Strength=125, Armor=none | — | Yes |
| Pip=white, OccupyPip=PersonBlue | — | Yes (occupy pip rendered) |
| Sight=5, Speed=4 | — | Yes |
| Owner=British,French,Germans,Americans,Alliance | — | Yes |
| IsSelectableCombatant=yes, ThreatPosed=10 | — | Yes |
| ImmuneToVeins=yes, ImmuneToPsionics=no | — | Yes |
| Bombable=yes, Crushable=yes | — | Yes |
| Deployer=yes, **DeployFire=yes** | false | **Partial — sequences exist but state machine is missing** |
| VeteranAbilities=STRONGER,FIREPOWER,ROF,SIGHT,FASTER | — | Partial (no kill counter) |
| EliteAbilities=SELF_HEAL,STRONGER,FIREPOWER,ROF | — | Partial |
| Size=1, PhysicalSize=1 | — | Yes |
| Locomotor={4A582744-…} (Walk) | — | Yes |
| MovementZone=Infantry | — | Yes |
| IFVMode=2 | 0 | Yes |
| Voice* (Select, Move, Attack, Feedback, SpecialAttack), DieSound, CrushSound, DeploySound, UndeploySound | — | Partial (Select/Move/Attack only) |
| Points=10 | — | Score reporting only |

**Section [GI] — artmd.ini lines 281–290** (Note: Image=GI redirects [E1] → [GI])

| Key | Value | Notes |
|-----|-------|-------|
| Cameo=GIICON, AltCameo=GIUICO | — | Used for sidebar |
| Sequence=GISequence | — | Animation table |
| Crawls=yes | false | Drives prone-while-moving |
| Remapable=yes | — | Remap palette |
| FireUp=2 | 0 | Frames before bullet spawns |
| PrimaryFireFLH=80,0,105 | — | Standing-fire muzzle FLH |
| SecondaryFireFLH=80,0,90 | — | Deployed-fire muzzle FLH |

**Weapons & warheads** — full surface in §3 of Agent B's scoping (cite in report).

**Base RA2 [E1] diff**: YR adds OccupyPip, OccupyWeapon, EliteOccupyWeapon,
OpenTransportWeapon, IFVMode; bumps Para Damage 15→25; bumps ParaE ROF 5→15,
Range 7→6; reduces Soylent 150→100; adds SecondaryFireFLH in artmd. Report
should note these so anyone using base RA2 doesn't get confused.

## 6. Caller & Integration Map

| Caller | Calls | When | Decompile? |
|--------|-------|------|------------|
| Tactical_ObjectRenderingLoop @ `0x6d8db0` | UpdateGarrisonFire | Every render frame for occupied buildings | YES — integration timing |
| Save / Load | InfantryClass ctors A & B | Map load + save restore | LIGHT — only confirm what's serialized |
| `FUN_00519710` (anon) | AddGarrisonOccupant | When GI right-clicks a civilian building | YES — name it; likely garrison-enter helper |
| InfantryClass::AI | Mission_Capture, Fire_At_Target, DoType, Locomotion_AI, Scatter, Fear_Decay_Handler | Every tick | All decompiled in Phase 1/2 |
| WarheadTypeClass::Detonate | Panic_SetFear300, CaptureManager::CaptureUnit | On warhead detonation that hits GI | MEDIUM — many warhead branches |
| ParaDropOverfly + ChronoSphere::WarpUnitsAtCell + Drop_Payload + SpawnSurvivors | PlaceInfantryInCell | Spawn paths | LIGHT — confirm spawn site only |
| Input handler | What_Action_OnCell, What_Action_OnObject | Right-click while GI selected | MEDIUM — drives cursor + voice |

**Rust integration notes** (from Agent C):
- GI flows through generic ObjectType parser today — most stats land correctly
- Combat firing path resolves Primary/Secondary/Elite + IFV via `select_weapon` —
  the deploy-fire weapon swap is the main hole
- Garrison fire exists with round-robin; needs GI's UCPara verification
- Sub-cell allocation uses `[0,3,4]` (BUG noted in INFANTRY_SUBCELL doc) — should
  be `[2,3,4]`
- Mission state machines for infantry are MISSING entirely (only aircraft missions exist)
- Mind control / panic / fear are MISSING entirely
- Veterancy promotion (kill-count → tier bump) is MISSING

The report should call these out in a "Rust Implementation Status" section so
the post-research `/brainstorm` / `/write-plan` can plan the gap-fill.

## 7. TS-Legacy Risk Register

GI is a YR-native unit with extensive use in skirmish — TS-legacy risk is uniformly
low. Two specific watch points:

1. **`SpecialFlags @ 0x008401d0`** — `InfantryClass::AI` references `g_RulesClass+0x344`
   (Pip cell anim). Some Pip-anim systems are TS-era. Verify the read isn't gated
   behind `SpecialFlags & 0x1000` (FogOfWar gate) or another off-default flag
   before treating it as live.
2. **`TriggerActionEntry__PlayVoiceForObjects @ 0x7265c0`** — campaign trigger
   voice playback. Skirmish does not use map triggers, so this should be ignored
   if encountered while tracing GI voice.
3. **`BuildingClass__SpawnSurvivors @ 0x442d90`** — survivor pool exists from TS;
   confirm Allied Barracks does NOT have `Survivor=GI` (it doesn't, by inspection,
   but the executor should verify the survivor field handling).
4. **Sequence 0x1F** — INFANTRYCLASS report mentions 0x1F as "CrawlUp completion",
   but YR sequences may have been renamed. Confirm the byte is reachable in YR.
5. **`SubCellDirOffset_Init @ 0x49f3b3`** — odd 4-aligned address, possibly
   inlined from a TS-era helper. Verify it's a real function before naming.

No `Magnetron`, `Chaos`, `Cloning`, `Squash`, `Subcell` strings exist in the
binary by name — these features all flow through generic systems (Locomotor,
Warhead, BuildingClass::Update, OnEnterCell). Don't go hunting for non-existent
labels.

## 8. Current Rust Implementation Surface

(From Agent C — verbatim file map; the executed report will use this to mark
each finding with **COVERED / PARTIAL / MISSING** in Rust.)

- `src/rules/object_type.rs` — InfantryType parsing (most keys covered)
- `src/rules/infantry_sequence.rs` — sequence parsing (Deploy/Deployed/DeployedFire/DeployedIdle present)
- `src/sim/game_entity.rs` — runtime entity (sub_cell, facing, veterancy, ifv_weapon_index)
- `src/sim/animation.rs` — sequence advancement + transitions
- `src/sim/combat/combat_weapon.rs` — Primary/Secondary/Elite/IFV/Garrison weapon selection
- `src/sim/combat/mod.rs` — combat dispatch + ProneDamage modifier (`apply_prone_damage_modifier`)
- `src/sim/combat/combat_fire_gate.rs` — empty-garrison firing block
- `src/sim/passenger.rs` — passenger/garrison vector
- `src/sim/movement/bump_crush.rs` — crush + sub-cell allocation (BUG: `[0,3,4]`)
- `src/render/sprite_atlas.rs` + `src/app_instances/shp.rs` — sprite batching
- `src/app_render/build_instances.rs` — pip rendering, selection bracket
- `src/sim/vision/mod.rs` — sight bonus on elite

**Known gaps / bugs to flag in the report:**
- Sub-cell array `[0,3,4]` should be `[2,3,4]`
- No deploy-fire state machine (sequences exist; transitions don't)
- No infantry mission system (Move/Attack/Guard/Hunt/Capture/Enter)
- No fear / panic / berserk / mind-control runtime
- No kill-count veterancy accumulator
- No Cloning Vats production
- No paradrop production trigger (sequence exists, spawn doesn't)
- VoiceFeedback / DieSound / CrushSound / DeploySound / UndeploySound not parsed

## 9. Deferred Open Questions

The scoping pass surfaced these — they become the "must answer in the executed report" list:

1. **What is the byte at 0x2A4?** Prone, deployed, or a packed flag for both?
2. **Does `Crawls=yes` flip GI to seq 0x1B (prone) only when receiving fire, or also when ordered to move?** YR docs disagree.
3. **Round-robin garrison fire:** does CurrentFireIdx advance per shot or per dead occupant? If GI elite + GI rookie are both inside, do shots alternate weapons?
4. **DeployFire weapon swap:** is the swap implemented in `Fire_At_Target` (picks Para based on IsDeployed) or in vtable +0x1EC `ActivateDeployFire` (mutates the weapon pointer)?
5. **Undeploy auto-trigger:** what input cancels deploy — any move command, only player move command, or also AI threat-flee?
6. **Panic vs Berserk precedence:** if GI is hit by Psychedelic warhead while already panicked, what wins?
7. **MC'd GI in a garrison:** can he be ejected by his original owner selling the building? Does the controller see the eject?
8. **Cloning Vats path:** by name there's no `Cloning` symbol — find the BuildingClass::Update branch that triggers free-GI spawn on InfantryFactory production.
9. **Sub-cell preference table directional bias:** does the original always prefer sub-cell from approach direction, or random?
10. **What_Action cursors:** for each pair (right-click target, GI state) what cursor is shown? (Move/Attack/Capture/Enter/Garrison/Disabled.)

## 10. Execution Strategy

**Recommended: Multi-phase /re-investigate, single executor session.**

- Run Phase 1 (functions #1–#11) in a single `/re-investigate` pass. Output:
  Phase 1 skeleton report — covers stats parse, AI loop, fire decision, damage,
  veterancy XP. Stop and present to user.
- After approval, run Phase 2 (functions #12–#36) in one or two passes depending
  on session-length budget. Output: full state machine (deploy/prone/panic),
  garrison fire, IFV, sub-cell, veterancy promotion, fear runtime.
- After approval, run Phase 3 (functions #37–#53). Output: spawn paths, mind
  control, crush-kill, render, voice, cursor logic.
- Final synthesis pass: write [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md) consolidating all three
  phases, adding the "Rust Implementation Status" section that maps each
  finding to COVERED/PARTIAL/MISSING.

**Subagent dispatch is NOT recommended** for this investigation — the GI's
behaviors are heavily cross-coupled (deploy interacts with fear, garrison
interacts with veterancy, sub-cell interacts with movement). Splitting across
parallel agents loses the integration context. Single-executor with phase
checkpoints is the right shape.

## 11. Success Criteria

The executed [GI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GI_GHIDRA_REPORT.md) must:

1. Answer all 10 questions in §1.
2. Resolve every conflict in §2 (5 items) or explicitly re-document as unresolved.
3. Resolve every deferred question in §9 (10 items) or explicitly re-document.
4. Include every function from §3 with: address, purpose, key findings, citation.
5. Decode every INI key on [E1] to its struct offset and runtime effect.
6. Include a state-machine diagram (text or table) covering all GI states +
   transitions + trigger functions.
7. Cite Ghidra addresses for every HIGH-confidence claim.
8. State "Active in YR: Yes/No/Conditional" for every finding.
9. Include a "Rust Implementation Status" appendix mapping each finding to
   COVERED / PARTIAL / MISSING with the relevant `src/` paths.
10. Total length: 1500–3000 lines (this is a feature-complete unit dossier;
    being concise is not a goal; being complete is).

---

## Sources

- **Ghidra addresses sampled:** see §3 (53 addresses).
- **Docs searched:**
  `docs/research/` (full archive scan, 17 reports identified as relevant)
- **INI files checked:**
  `ini/rulesmd.ini` (lines 3713–3759 + 22922–22964 + 24366–24375 + 25281–25301 + 26466–26527),
  `ini/artmd.ini` (lines 281–290 + 16131–16248),
  `ini/rules.ini` (lines 3130+),
  `ini/art.ini` (lines 213+)
- **Rust files checked:** see §8.
- **Related plans:** none (no prior plan on GI specifically).
