# Rules Process Native-ID Constructor Chronology — Ghidra Re-investigation

**Address(es):** `RulesClass::Process @ 0x00668BF0`, `RulesClass::ReadTypeData @ 0x00679A10`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** every active-YR Type constructor path that can call `AbstractClass::AssignUniqueID` during one `RulesClass::Process` pass, including fixed Art input and ordered post-type global readers  
**Non-Scope:** process pre-reset ownership, House/Super instance blocks, Cell resize, preview generation, map-reader reservation, Tubes, Overlay/object instance construction, and save/checksum persistence  
**Confidence:** High for the complete constructor-bearing call graph, family/key order, live-loop timing, name/list semantics, and fixed-Art source; stock fresh-versus-repeat classification is recorded separately where data proves it  
**Active in YR:** Yes — the fresh `Full_Init` rules stack calls this owner for base/language/mode/map sources

## 1. Overview

This gap extension was opened because the prior prefix report correctly required
an ordered first-new constructor stream but described the current Rust producer as
preserving only “most” lazy timing. A fresh implementation audit proved that the
missing timing is material: constructor-capable reads occur before
`ReadTypeData`, throughout inherited and subclass type readers, through the
fixed Art object, and after all type loops. Several of those readers append to a
registry whose live count is still being walked; others append to a family whose
body loop has already finished. Those two cases have different same-pass
behavior and cannot be reconstructed from final registry lengths or a merged
INI.

**Verdict:** one exact `RulesClass::Process` contribution is the ordered stream
of successful first-new constructors described in this report. Its authority is
an ordered, process-resident registry of the native stored identifiers for every
ID-bearing Type family, plus a fixed active-YR Art INI. The compatibility
`RuleSet` and merged Rules INI are downstream projections and cannot be the
native-ID authority.

The prior implementation draft is therefore not certifiable. It correctly made
the receipt move-only and separated explicit Side identity from its membership
projection, but it omits or misorders constructor paths, uses full untruncated
names as registry identity, does not receive fixed Art, and discards the receipt
at the production load boundary.

## 2. Native owner, inputs, and event model

### 2.1 Owner and pass stack

`RulesClass::Process @ 0x00668BF0` mutates process-lifetime Type registries. A
successful first allocation calls that family's constructor; all 16 families in
scope call `AbstractClass::AssignUniqueID`. Registry contents survive from one
Rules pass to the next until the explicit rules reset. Therefore every pass must
receive and return the same ordered registry state.

The active YR fresh-load stack is:

1. `RULESMD.INI`;
2. optional `LANGRULE.INI`;
3. the selected nonzero-mode override INI when present;
4. the scenario/map INI.

RA2 `RULES.INI` is not a base layer in this active `gamemd.exe` path. The Art
argument is not a Rules layer: `ReadTypeData @ 0x00679A10` reads the single fixed
global `g_ArtINI @ 0x00887180` during every pass. Active YR loads this from
`ARTMD.INI`; language, mode, and scenario Rules passes do not flatten their
sections into that Art object.

### 2.2 ID-bearing families

The complete Type-family set in this slice is:

| Native family | Representative constructor | Assigns an ID? | Included in event receipt? |
|---|---:|---:|---:|
| AircraftType | `0x0041C8B0` | yes | yes |
| AnimType | `0x00427530` | yes | yes |
| BuildingType | `0x0045DD90` | yes | yes |
| BulletType | `0x0046BBC0` | yes | yes |
| HouseType | `0x005113F0` | yes | yes |
| InfantryType | `0x005236A0` | yes | yes |
| OverlayType | `0x005FE250` | yes | yes |
| ParticleSystemType | `0x006440A0` | yes | yes |
| Side | `0x006A4550` | yes | yes |
| SmudgeType | `0x006B5260` | yes | yes |
| SuperWeaponType | `0x006CE5B0` | yes | yes |
| TerrainType | `0x0071DA80` | yes | yes |
| UnitType | `0x007470D0` | yes | yes |
| VoxelAnimType | `0x0074AD80` | yes | yes |
| WarheadType | `0x0075CEC0` | yes | yes |
| WeaponType | `0x00771C70` | yes | yes |

`ParticleTypeClass` is constructor-capable and participates in live body reads,
but its constructor never calls Assign. `TiberiumClass`, MissionControl,
ScriptType, TeamType, TaskForce, TriggerType, TagType, AITriggerType, and
IsometricTileType likewise contribute no event to this Type-ID receipt. Some of
them can still *cause* an ID-bearing child allocation; Tiberium is the important
late example.

### 2.3 What one event means

One receipt event is emitted immediately after a successful constructor that
calls Assign, in native call order. It records:

- the native family;
- the native stored identifier, not an unbounded source spelling;
- no synthetic ID value: the consuming shared wrapping signed-dword cursor owns
  the actual next ID.

No event is emitted for a case-insensitive repeat, an empty value, a native none
sentinel, a non-ID Particle allocation, or an allocation failure. A safe Rust
implementation may hard-error on allocation failure instead of emulating the
native partially degraded registry, but it may not emit a successful event.

## 3. Exact constructor chronology

### 3.1 Top-level `RulesClass::Process` order

The constructor-bearing stages, with zero-event readers retained as ordering
boundaries, are:

```text
ReadColors -> ColorAdd                   // no Type factory
-> explicit Countries, Sides, OverlayTypes, SuperWeaponTypes, Warheads,
SmudgeTypes, TerrainTypes, BuildingTypes, VehicleTypes, AircraftTypes,
InfantryTypes, Animations, VoxelAnims, Particles, ParticleSystems
-> ReadJumpjetControls                 // no Type factory
-> ReadMultiplayer                     // no Type factory
-> [AI] reader @ 0x00672AE0           // Building factories
-> ReadPowerups                        // lookup/string work only
-> ReadSpeed/LandCharacteristics       // no Type factory
-> ReadIQ                              // no Type factory
-> ReadGeneral @ 0x0066D530           // mixed Type factories
-> ReadTypeData @ 0x00679A10
-> difficulty readers                  // no Type factory
-> Crate -> Combat -> Radiation -> Elevation -> Wall -> AudioVisual
-> SpecialWeapons -> Tiberium All
-> Advanced/MultiplayerAdvancedCommandBar // no Type factory
```

The earlier `0x0066F790` often cited for General is an interior address. The
reader entry is `0x0066D530`.

### 3.2 Explicit registry phase

| Process section | Identity supplied to factory | Family | Order note |
|---|---|---|---|
| `[Countries]` | entry value, `ReadString` cap `0x20` | HouseType | section source-entry order |
| `[Sides]` | entry **key**, not membership value | Side | source-entry order; membership is lookup-only |
| `[OverlayTypes]` | entry value | OverlayType | source-entry order |
| `[SuperWeaponTypes]` | entry value | SuperWeaponType | source-entry order |
| `[Warheads]` | entry value | WarheadType | source-entry order |
| `[SmudgeTypes]` | entry value | SmudgeType | source-entry order |
| `[TerrainTypes]` | entry value | TerrainType | source-entry order |
| `[BuildingTypes]` | entry value | BuildingType | source-entry order |
| `[VehicleTypes]` | entry value | UnitType | source-entry order |
| `[AircraftTypes]` | entry value | AircraftType | source-entry order |
| `[InfantryTypes]` | entry value | InfantryType | source-entry order |
| `[Animations]` | entry value | AnimType | source-entry order |
| `[VoxelAnims]` | entry value | VoxelAnimType | source-entry order |
| `[Particles]` | entry value | ParticleType | constructs but costs no ID |
| `[ParticleSystems]` | entry value | ParticleSystemType | source-entry order |

`FUN_00672440` proves the Side exception: stock rows such as
`GDI=British,French,...` construct `Side("GDI")`; the value tokens only resolve
existing HouseTypes and Sides through `FUN_004767C0` and construct no HouseType.
`HouseTypeClass::ReadINI` may independently allocate a Side later from a
HouseType's `Side=` value.

The retail `RULESMD.INI` explicit subtotal remains the previously verified
1,699 ID-bearing constructor events on empty family registries: 1,704 rows less
the repeated `NAPSYA`, `GAWEAP_1`, `GAWEAP_2`, `GAWEAP_A`, and `TWLT100` names.
The 22 `[Particles]` rows are outside both totals because Particle has no Assign.
This is only the explicit subtotal; it is not the complete pass receipt.

### 3.3 Pre-type `[AI]` and `[General]`

#### `[AI] @ 0x00672AE0`

The reader applies BuildingType list factories in this exact key order:

```text
BuildConst -> BuildPower -> BuildRefinery -> BuildBarracks -> BuildTech
-> BuildWeapons -> AlliedBaseDefenses -> SovietBaseDefenses
-> ThirdBaseDefenses -> BuildDefense -> BuildPDefense -> BuildAA
-> BuildHelipad -> BuildRadar -> ConcreteWalls -> NSGates -> EWGates
-> BuildNavalYard -> BuildDummy -> NeutralTechBuildings
```

#### `[General] @ 0x0066D530`

The direct constructor sequence is exactly:

| # | Key | Family / shape |
|---:|---|---|
| 1 | `DamageFireTypes` | Anim list |
| 2 | `OreTwinkle` | Anim scalar |
| 3 | `BarrelExplode` | Anim scalar |
| 4 | `BarrelDebris` | VoxelAnim list |
| 5 | `BarrelParticle` | ParticleSystem scalar |
| 6 | `NukeTakeOff` | Anim scalar |
| 7 | `Wake` | Anim scalar |
| 8 | `DropPod` | Anim list |
| 9 | `DeadBodies` | Anim list |
| 10 | `MetallicDebris` | Anim list |
| 11 | `BridgeExplosions` | Anim list |
| 12 | `IonBlast` | Anim scalar |
| 13 | `IonBeam` | Anim scalar |
| 14 | `WeatherConClouds` | Anim list |
| 15 | `WeatherConBolts` | Anim list |
| 16 | `WeatherConBoltExplosion` | Anim scalar |
| 17 | `DominatorWarhead` | Warhead scalar |
| 18 | `DominatorFirstAnim` | Anim scalar |
| 19 | `DominatorSecondAnim` | Anim scalar |
| 20 | `ChronoPlacement` | Anim scalar |
| 21 | `ChronoBeam` | Anim scalar |
| 22 | `ChronoBlast` | Anim scalar |
| 23 | `ChronoBlastDest` | Anim scalar |
| 24 | `WarpIn` | Anim scalar |
| 25 | `WarpOut` | Anim scalar |
| 26 | `WarpAway` | Anim scalar |
| 27 | `IronCurtainInvokeAnim` | Anim scalar |
| 28 | `ForceShieldInvokeAnim` | Anim scalar |
| 29 | `WeaponNullifyAnim` | Anim scalar |
| 30 | `ChronoSparkle1` | Anim scalar |
| 31 | `InfantryExplode` | Anim scalar |
| 32 | `FlamingInfantry` | Anim scalar |
| 33 | `InfantryHeadPop` | Anim scalar |
| 34 | `InfantryNuked` | Anim scalar |
| 35 | `InfantryVirus` | Anim scalar |
| 36 | `InfantryBrute` | Anim scalar |
| 37 | `InfantryMutate` | Anim scalar |
| 38 | `Behind` | Anim scalar |
| 39 | `MoveFlash` | Anim scalar |
| 40 | `Parachute` | Anim scalar |
| 41 | `BombParachute` | Anim scalar |
| 42 | `DropZoneAnim` | Anim scalar |
| 43 | `EMPulseSparkles` | Anim scalar |
| 44 | `LargeVisceroid` | Unit scalar |
| 45 | `SmallVisceroid` | Unit scalar |
| 46 | `DropPodWeapon` | Weapon scalar |
| 47 | `ExplosiveVoxelDebris` | VoxelAnim list |
| 48 | `TireVoxelDebris` | VoxelAnim scalar |
| 49 | `ScrapVoxelDebris` | VoxelAnim scalar |
| 50 | `RepairBay` | Building list |
| 51 | `GDIGateOne` | Building scalar |
| 52 | `GDIGateTwo` | Building scalar |
| 53 | `NodGateOne` | Building scalar |
| 54 | `NodGateTwo` | Building scalar |
| 55 | `WallTower` | Building scalar |
| 56 | `Shipyard` | Building list |
| 57 | `GDIPowerPlant` | Building scalar |
| 58 | `NodRegularPower` | Building scalar |
| 59 | `NodAdvancedPower` | Building scalar |
| 60 | `ThirdPowerPlant` | Building scalar |
| 61 | `PrerequisiteProcAlternate` | Unit scalar |
| 62 | `BaseUnit` | Unit list |
| 63 | `HarvesterUnit` | Unit list |
| 64 | `PadAircraft` | Aircraft list |
| 65 | `Paratrooper` | Infantry scalar |
| 66 | `SecretInfantry` | Infantry list |
| 67 | `SecretUnits` | Unit list |
| 68 | `SecretBuildings` | Building list |
| 69 | `AlliedDisguise` | Infantry scalar |
| 70 | `SovietDisguise` | Infantry scalar |
| 71 | `ThirdDisguise` | Infantry scalar |
| 72 | `Engineer` | Infantry scalar |
| 73 | `Technician` | Infantry scalar |
| 74 | `Pilot` | Infantry scalar |
| 75 | `AlliedCrew` | Infantry scalar |
| 76 | `SovietCrew` | Infantry scalar |
| 77 | `ThirdCrew` | Infantry scalar |
| 78 | `AmerParaDropInf` | Infantry list |
| 79 | `AllyParaDropInf` | Infantry list |
| 80 | `SovParaDropInf` | Infantry list |
| 81 | `YuriParaDropInf` | Infantry list |
| 82 | `AnimToInfantry` | Infantry list |
| 83 | `LightningWarhead` | Warhead scalar |
| 84 | `PrismType` | Building scalar |
| 85 | `V3RocketType` | Aircraft scalar |
| 86 | `DMislType` | Aircraft scalar |
| 87 | `CMislType` | Aircraft scalar |
| 88 | `VeinholeTypeClass` | Terrain scalar |
| 89 | `DefaultMirageDisguises` | Terrain list |

`Prerequisite_INI_Parser @ 0x004770E0` is lookup-only against BuildingType and
does not add events. A similarly named prerequisite field must not be promoted
into this constructor ledger without a factory call.

### 3.4 `ReadTypeData @ 0x00679A10`

The family loop order is:

```text
House -> SuperWeapon -> Anim(fixed Art) -> Building -> Aircraft -> Unit
-> Infantry -> Weapon -> Bullet -> Warhead -> Weapon post -> Building post
-> Terrain -> Smudge -> Overlay -> Particle -> ParticleSystem -> VoxelAnim
-> MissionControl
```

Every family loop compares its index with the **current live count** on every
iteration. The per-object constructor-capable reads are below.

#### House and SuperWeapon

| Reader | Exact constructor order |
|---|---|
| `HouseTypeClass::ReadINI @ 0x00511850` | `VeteranInfantry` -> Infantry list; `VeteranUnits` -> Unit list; `VeteranAircraft` -> Aircraft list; `Side` -> Side scalar |
| `SuperWeaponTypeClass::ReadINI @ 0x006CEA20` | `WeaponType` -> Weapon; `AuxBuilding` -> Building |

All of their target family loops are still ahead, so a newly appended stock or
custom type receives its body later in this same pass.

#### Anim using fixed Art

`AnimTypeClass::ReadINI @ 0x00427D00` receives `g_ArtINI`, not the current Rules
pass. Its exact constructor order is:

```text
Next -> Anim
Spawns -> Anim
TiberiumSpawnType -> Overlay
BounceAnim -> Anim
ExpireAnim -> Anim
TrailerAnim -> Anim
Warhead -> Warhead
SpawnsParticle -> Particle             // no ID event
```

Because the Anim loop is live, `Next`, `Spawns`, `BounceAnim`, `ExpireAnim`, or
`TrailerAnim` can append another Anim whose own fixed-Art body is then read in
the same sweep. Overlay, Warhead, and Particle loops are later and also receive
same-pass bodies.

#### Techno inheritance and subclasses

`TechnoTypeClass::ReadINI @ 0x00712170` runs before each Building, Aircraft,
Unit, and Infantry subclass reader. Its constructor order is:

1. `DeathWeapon` -> Weapon;
2. `DebrisTypes` -> VoxelAnim list;
3. `DebrisAnims` -> Anim list;
4. one conditional weapon-bank block:
   - when `TurretCount >= 1 && WeaponCount > 0`, for each one-based slot:
     `Weapon{i}` then `EliteWeapon{i}`;
   - otherwise, when `TurretCount < 1 && ClearAllWeapons == false`:
     `Primary`, `Secondary`, `ElitePrimary`, `EliteSecondary`;
   - otherwise no weapon-bank factories;
5. `Dock` -> Building list;
6. `DeploysInto` -> Building;
7. `UndeploysInto` -> Unit;
8. `PowersUnit` -> Unit;
9. `Explosion` -> Anim list;
10. `DestroyAnim` -> Anim list;
11. `NaturalParticleSystem` -> ParticleSystem;
12. `RefinerySmokeParticleSystem` -> ParticleSystem;
13. `DamageParticleSystems` -> ParticleSystem list;
14. `DestroyParticleSystems` -> ParticleSystem list;
15. `AirstrikeTeamType` -> Aircraft;
16. `EliteAirstrikeTeamType` -> Aircraft;
17. `UnloadingClass` -> Unit;
18. `DeployingAnim` -> Anim;
19. `Enslaves` -> Infantry through `0x0067BAC0`;
20. `Spawns` -> Aircraft through `0x0067BD30`.

The conditional weapon-bank ordering is data-dependent and must be evaluated
from the object's effective values at that point in the pass; a generic union of
all possible weapon keys is wrong.

Subclass tails run immediately after that inherited sequence:

| Reader | Exact subclass tail |
|---|---|
| Building `0x0045FE50` | Rules `FreeUnit` -> Unit; Rules `SecretInfantry` -> Infantry; Rules `SecretUnit` -> Unit; Rules `SecretBuilding` -> Building; fixed-Art `ToOverlay` -> Overlay |
| Aircraft `0x0041CC20` | fixed-Art `Trailer` -> Anim |
| Unit `0x00747620` | no subclass Type factory |
| Infantry `0x005240A0` | `OccupyWeapon` -> Weapon; `EliteOccupyWeapon` -> Weapon; `DeadBodies` -> Anim list; `DeathAnims` -> Anim list |

#### Weapon, Bullet, and Warhead

`WeaponTypeClass::ReadINI @ 0x00772080` is **member-major**, not family-batched:

```text
Anim -> Anim list
AssaultAnim -> Anim
OccupantAnim -> Anim
OpenToppedAnim -> Anim
AttachedParticleSystem -> ParticleSystem
Warhead -> Warhead
Projectile -> Bullet
```

Thus two live weapons produce the complete sequence for weapon 0 before weapon
1. A processor that emits all bullets, then all warheads, then all particle
systems reverses native chronology.

`BulletTypeClass::ReadINI @ 0x0046BEE0` performs:

1. read Rules `Image`;
2. when nonempty, select fixed Art section `[Image]` and read `Trailer` -> Anim;
3. return to Rules and read `AirburstWeapon` -> Weapon;
4. read `ShrapnelWeapon` -> Weapon.

Weapon and Anim loops are already complete at this point, so those children get
no body until a later Rules pass.

The full `WarheadTypeClass::ReadINI` body starts at `0x0075D3A0`; the often-cited
`0x0075D590` is an interior/overlapping artifact. Its constructor order is:

```text
Particle -> ParticleSystem
AnimList -> Anim list
DebrisTypes -> VoxelAnim list
```

ParticleSystem and VoxelAnim bodies occur later this pass; Anim bodies wait for
a later pass.

#### Remaining type readers

| Reader | Constructor result |
|---|---|
| Weapon post `0x007729F0` | numeric/post fields only; no factory |
| Building post `0x00465CB0` | immediate return; no factory |
| Terrain | no ID-bearing factory |
| Smudge `0x006B56D0` | no Type factory |
| Overlay | no ID-bearing factory |
| Particle `0x00644F50` | `Warhead` -> Warhead |
| ParticleSystem `0x006442D0` | `HoldsWhat` -> Particle, which has no ID |
| VoxelAnim `0x0074B050` | `BounceAnim` -> Anim; `ExpireAnim` -> Anim; `TrailerAnim` -> Anim; `Warhead` -> Warhead; `AttachedSystem` -> ParticleSystem |
| MissionControl | no ID-bearing factory |

Particle-created Warheads and every VoxelAnim-created ID-bearing child target a
family whose body loop has already run; their bodies wait for the next Rules
pass.

### 3.5 Post-type readers

The exact top-level order and constructor-capable keys are:

#### CrateRules `0x0066B900`

```text
WoodCrateImg -> Overlay
CrateImg -> Overlay
WaterCrateImg -> Overlay
UnitCrateType -> Unit
```

#### CombatDamage `0x0066BBB0`

```text
Scorches, Scorches1, Scorches2, Scorches3, Scorches4 -> Smudge lists
SplashList -> Anim list
FlameDamage -> Warhead
FlameDamage2 -> Warhead
C4Warhead -> Warhead
CrushWarhead -> Warhead
V3Warhead -> Warhead
DMislWarhead -> Warhead
V3EliteWarhead -> Warhead
DMislEliteWarhead -> Warhead
CMislWarhead -> Warhead
CMislEliteWarhead -> Warhead
IvanWarhead -> Warhead
DeathWeapon -> Weapon
DrainAnimationType -> Anim
ControlledAnimationType -> Anim
PermaControlledAnimationType -> Anim
IonCannonWarhead -> Warhead
DefaultLargeGreySmokeSystem -> ParticleSystem
DefaultSmallGreySmokeSystem -> ParticleSystem
DefaultSparkSystem -> ParticleSystem
DefaultLargeRedSmokeSystem -> ParticleSystem
DefaultSmallRedSmokeSystem -> ParticleSystem
DefaultDebrisSmokeSystem -> ParticleSystem
DefaultFireStreamSystem -> ParticleSystem
DefaultTestParticleSystem -> ParticleSystem
DefaultRepairParticleSystem -> ParticleSystem
```

#### Radiation, Elevation, Wall, and AudioVisual

- Radiation `0x0066CF70`: `RadSiteWarhead` -> Warhead.
- Elevation `0x0066D150`: no factory.
- Wall `0x0066D1F0`: no factory.
- AudioVisual `0x006691E0`, exact order:
  `DropPodPuff`, `VeinAttack`, `Dig`, `AtmosphereEntry` -> Anim scalars;
  `TreeFire`, `OnFire` -> Anim lists; `Smoke` -> Anim scalar twice into distinct
  fields; `SmallFire`, `LargeFire` -> Anim scalars.

The two `Smoke` reads are two factory lookups. The first may construct; the
second is normally a repeat and costs zero. It must not be collapsed merely
because the INI key spelling repeats.

#### SpecialWeapons `0x00668FB0`

The family interleave is exact:

```text
NukeWarhead -> Warhead
NukeProjectile -> Bullet
NukeDown -> Bullet
MutateWarhead -> Warhead
MutateExplosionWarhead -> Warhead
EMPulseWarhead -> Warhead
EMPulseProjectile -> Bullet
```

`ParaDropPlane` is not read here and is not a native constructor row. After the
seven keys, if `[SpecialWeapons]` exists, native iterates the then-live Warhead
registry and explicitly rereads every Warhead body through its `+0x64` vslot.
That reread uses the Warhead order above (`Particle`, `AnimList`,
`DebrisTypes`). It can create new ParticleSystems and VoxelAnims immediately;
their ordinary type loops have already passed, and any Anim child also waits for
the next Rules pass.

#### Tiberium All `0x00721D10`

The owner snapshots `[Tiberiums]` row count. For each row it reads at most
`0x18` bytes of value. Empty rows are skipped. A nonnegative numeric slot less
than the current count selects that object and ignores the row name; a slot at
or beyond the current count creates exactly one non-ID-bearing Tiberium object,
without filling sparse gaps or deduplicating the supplied name. A negative slot
performs invalid native pre-array indexing and can fault; safe Rust should reject
it. Allocation failure can likewise be a hard error.

Every selected/new object immediately runs `TiberiumClass::ReadINI @
0x00721A50`. `Image=` is an ordinal Overlay lookup and allocates nothing;
`Debris=` is an ordered Anim token list and can append ID-bearing AnimTypes.
Those Anim bodies wait until a later Rules pass because the Anim loop is long
finished.

The final advanced-command-bar helper `FUN_00674650` reads button-list state
only. It has no Type factory and stock YR ships neither selected command-bar
section, so it contributes zero events.

## 4. Native factory, string, and list semantics

### 4.1 Stored identifier truncation changes identity

`AbstractTypeClass::Constructor @ 0x00410800` copies the incoming identifier
through `_strncpy(..., 0x18)` and writes a terminator. Every family in scope,
including `SideClass::Constructor @ 0x006A4550`, inherits this storage. The
native stored Type ID is therefore at most 24 bytes.

Factories scan the ordered registry and compare each stored ID
case-insensitively against the **full incoming lookup string**. This has a
counterintuitive but verified consequence: a source identifier longer than 24
bytes is stored truncated; a later lookup of the same long source spelling does
not match that truncated stored ID and may allocate another object with the same
24-byte stored ID. A full-name `HashMap` incorrectly deduplicates that case.

The exact registry model is consequently an ordered vector of 24-byte native
stored IDs per family. Lookup compares every stored value against the raw
post-`ReadString`/post-token input; construction stores the first 24 bytes; the
event records that stored value. Compatibility projections may retain their
current full strings separately, but they are not constructor authority.

The explicit master-list value buffer is `0x20` bytes (31 characters plus
terminator) before constructor truncation. Common lazy scalar reads use `0x80`;
known exceptions include Weapon `AttachedParticleSystem` at `0x14` (19 usable
characters), Bullet `Image` at `0x19` (24 usable), ParticleSystem `HoldsWhat` at
`0x40`, Anim `SpawnsParticle` at `0x20`, and a Tiberium master-row value at
`0x18` (23 usable).

### 4.2 Empty, none, repeat, and allocation outcomes

The generic Type factories:

- treat `none` and `<none>` case-insensitively as null/no allocation;
- scan existing stored IDs case-insensitively in registry order;
- allocate and append only on a nonempty non-sentinel miss;
- emit no event on repeat or sentinel;
- emit one event only after successful construction/Assign.

Side is the verified exception to the sentinel rule. `FUN_004756F0` reads the
value and calls the plain Side registry scan `FUN_006A46D0`; neither rejects
`none` or `<none>`. A nonempty Side miss with either spelling therefore
constructs a Side and spends an ID. The explicit `[Sides]` key path has the same
plain lookup behavior. Global sentinel filtering across all families is wrong.

Assign occurs inside the constructor before the later family-registry join. A
constructor can therefore spend an ID even if that subsequent join fails; a
retry may spend again because the object is not findable in the family vector.
Exact Rust need not emulate partial OOM corruption, but its deliberate policy is
to hard-error at that boundary, never to continue with a different event stream.

`CCINIClass::ReadString` trims the whole scalar string. Missing, empty, or
whitespace-only input does not call the factory and retains the prior scalar or
vector state for an overlay pass. A nonempty sentinel or allocation-null result
stores null for a scalar. A safe implementation may report OOM instead of
retaining native null state.

### 4.3 Lists are not Rust-style comma lists

Native list readers use `strtok(..., ",")`. Consequences:

- empty fields collapse;
- individual tokens are **not** trimmed after splitting;
- a token that resolves null is skipped and iteration continues;
- a repeated existing Type pointer is still appended to the destination vector;
- a missing/empty whole key retains the prior vector on later Rules passes.

Trimming each token, deduplicating vectors, or clearing an absent later key can
therefore change both compatibility data and constructor chronology.

## 5. Growing-loop body timing

Every `ReadTypeData` family loop reloads live count. The timing rule is:

- child appended to the family currently being walked -> body read later in the
  same live loop;
- child appended to a family whose loop is still ahead -> body read later in
  this pass;
- child appended to a family whose loop has passed -> body waits for the next
  Rules pass.

Concrete ledger:

| Parent reader | Same-pass child bodies | Later-pass child bodies |
|---|---|---|
| House | Infantry, Unit, Aircraft | none of its verified targets |
| SuperWeapon | Building, Weapon | none |
| Anim | Anim recursively; Overlay, Warhead, Particle later | none of its verified targets |
| Building | Building recursively; Aircraft, Unit, Infantry, Weapon, Bullet, Warhead, Particle/System, Voxel later as reached through inheritance/tail | Anim |
| Aircraft | Aircraft recursively; Unit, Infantry, Weapon and later families | Building, Anim |
| Unit | Unit recursively; Infantry, Weapon and later families | Building, Aircraft, Anim |
| Infantry | Infantry recursively; Weapon and later families | Building, Aircraft, Unit, Anim |
| Weapon | Bullet, Warhead, ParticleSystem and later families | Anim |
| Bullet | none for its constructor targets | Anim and Weapon |
| Warhead | ParticleSystem and VoxelAnim | Anim |
| Particle | none | Warhead |
| ParticleSystem | Particle body is later in registry order? no: Particle loop has already passed | Particle (non-ID body only) |
| VoxelAnim | none | Anim, Warhead, ParticleSystem |
| post-type readers | no ordinary type loop remains | every new Type body, except the explicit SpecialWeapons Warhead reread described above |

This is why final lengths cannot reconstruct the receipt. Two direct
counterexamples are sufficient:

1. A Bullet appends a Weapon after the Weapon loop. Its ID is spent immediately,
   but its Projectile/Warhead/ParticleSystem children do not exist until the
   next pass.
2. A new post-Special Warhead is followed by an explicit live Warhead reread;
   registry length alone cannot say which ParticleSystem/VoxelAnim/Anim events
   occurred in that tail or where they interleaved.

## 6. Active-retail data and exclusions

### 6.1 Stock activation boundary

Stock `ini/rulesmd.ini` contains the active `[AI]`, `[General]`, type-body,
CrateRules, CombatDamage, Radiation, AudioVisual, SpecialWeapons, and Tiberiums
sections. Stock `ini/artmd.ini` supplies the fixed Anim/Bullet Art bodies. A key
with TS-era naming is not dormant merely because its gameplay feature looks
legacy: if this active `gamemd.exe` call graph reads a nonempty stock value, its
constructor event remains part of the fresh prefix.

The exact stock fresh-versus-repeat classification is data- and order-dependent:
the 1,699 explicit events seed most named families, while Weapon and Bullet have
no numbered explicit master list and are normally born from lazy references.
The implementation oracle must therefore run the verified algorithm over the
actual ordered `RULESMD.INI` plus fixed `ARTMD.INI`; hard-coded retail counts are
tests, not production authority.

The bounded stock activation audit establishes these load-bearing checkpoints:

- `[AI]` sites 1-9 and 14-20 are present; `BuildDefense`, `BuildPDefense`,
  `BuildAA`, and `BuildHelipad` are absent. Every active token already exists in
  `[BuildingTypes]`, so stock `[AI]` emits zero new events. RA2 `RULES.INI` has a
  different 17-of-20 activation pattern and must not be merged underneath YR.
- 88 of the 89 `[General]` sites are present. The binary reads `Paratrooper`,
  while retail has the different key `PParatrooper`; that one site is inactive.
- after the explicit registries, stock General emits exactly five new events in
  this order: Anim `D`, created because `CCINIClass::ReadString @ 0x00528A10`
  copies at most 127 characters for the `0x80`-byte `MetallicDebris` caller at
  `RulesClass::ReadGeneral @ 0x0066D530`, leaving the final partial token `D`;
  Anim `WCLBOLT2` from the middle `WeatherConBolts` token; Unit `VISC_LRG`;
  Unit `VISC_SML`; Weapon `Vulcan2` from `DropPodWeapon`. The earlier four-event
  classification incorrectly reasoned from the untruncated retail value.
- `Visceroids=no`, dormant TS DropPod gameplay, and legacy-looking key names do
  not suppress those unconditional load-time parser calls.
- fixed Art and the later member-major readers add further events that an
  explicit-only recount misses. Verified stock examples include Anim `SMOKEY2`
  from an Art `TrailerAnim`, Anims `YURICNTL` then `APMUZZLE` from Weapon bodies,
  and Anim `BBBLELRG` from the active Torpedo Bullet-image Art `Trailer` read.
  `DURASMOKE` is a live custom-data possibility through `[DREDMISS] Trailer=`,
  but stock `DredMissile` is referenced only by the unreferenced
  `[DredCollision]` Weapon body, so it is not constructed by the stock Rules
  pass. The earlier stock classification incorrectly treated the ART row itself
  as reachability evidence.
- stock Building bodies freshly allocate Anims in registry order:
  `gtpowexp` (`GACNST`), `tstlexp` (`NAPOWR`), `CAWA15DM`, then `CACH06DM`;
  `YAPOWR` later repeats `gtpowexp`. Warhead-body processing freshly allocates Anim
  `APOCEXP`. The retail row `AnimList=MININUKE - added 11/30` has no semicolon:
  native comma tokenization treats the complete 22-character value as a real,
  fresh Anim identifier rather than a comment.
- stock Art contains `Next`, `Spawns`, `TiberiumSpawnType`, `ExpireAnim`,
  `TrailerAnim`, `Warhead`, `SpawnsParticle`, and Bullet `Trailer` assignments.
  No stock `BounceAnim=` assignment was found, so that live parser path is
  custom-data-only for the stock corpus.
- stock Rules contains active examples of the inherited Techno, Weapon, Bullet,
  Warhead, Particle, ParticleSystem, and Tiberium-Debris paths. Several optional
  keys are absent in stock (for example House veteran lists, SuperWeapon
  `AuxBuilding`, `NaturalParticleSystem`, `DestroyParticleSystems`,
  `SecretUnit`, `SecretBuilding`, VoxelAnim `AttachedSystem`), but their native
  readers are active and remain required for mode/map/custom inputs.
- stock AudioVisual has no `Dig=` Anim value (its `DigSound=` is a separate Voc
  field); `Smoke=xxxx` is its only fresh constructor result, with the second
  Smoke lookup repeating it. Stock Combat freshly adds Anim `MINDANIMR`.
  SpecialWeapons freshly adds Bullets `NukeUp` and `NukeDown`; `PulsPr` is an
  earlier repeat. Crate, Radiation, fixed-Art Building `ToOverlay`, and
  Tiberium `CRYSTAL1..4` Debris targets are repeats or native-none in stock.

### 6.2 Explicit exclusions

Excluded from this receipt, with reason:

- `RULES.INI` / `ART.INI`: RA2 roots, not active-YR base layers;
- `[Particles]` / `SpawnsParticle` / `HoldsWhat`: Particle constructors spend no
  native ID, though their bodies can cause later ID-bearing children;
- Weapon post, Building post, Terrain, Smudge, Overlay, MissionControl,
  Elevation, Wall, and the other pre-General readers: audited zero ID-bearing
  factories;
- Side membership values: lookup-only; Side identity is the `[Sides]` entry key;
- prerequisite parser Building references: lookup-only;
- Tiberium and its `Image=` selection: non-ID Tiberium constructor and direct
  Overlay ordinal lookup; only its `Debris=` Anim list contributes events;
- Script/Team/TaskForce/Trigger/Tag/AITrigger Type constructors: no Assign and
  outside this Process owner;
- runtime object constructors, House/Super instance blocks, Cell resize, Tubes,
  preview/map readers, and Overlay/Anim instances: owned by adjacent prefix
  transactions, not this Type pass.

Custom/mode/map data may activate any constructor-capable key in Sections 3.3 to
3.5 because the active YR binary executes those readers. “Absent in stock” is
not permission to omit a verified live parser path from an exact general receipt.

## 7. Current Rust mismatch

The in-flight `RulesPassProcessor` has sound scaffolding but remains incomplete:

| Area | Current state | Required correction |
|---|---|---|
| ownership | move-only `NativeTypeConstructionTrace` carried through `ProcessedRulesLayers` and `LoadedRules` | keep; move once into process-resident prefix state instead of discarding in `init.rs` |
| explicit phase | correct family order; Side key separated from projection; Particle excluded from events | retain, but use ordered native stored-ID registry and exact source-buffer behavior |
| identity | full untruncated Rust names in map/set-like registry | replace constructor authority with ordered 24-byte stored IDs and full-input comparison |
| fixed Art | processor receives Rules layers only | load/pass one fixed `ARTMD.INI` before Rules processing for Anim bodies, Aircraft `Trailer`, Building `ToOverlay`, and Bullet-image `Trailer`; never merge it into Rules projection |
| pre-type | partial General; no exact `[AI]`; many General omissions | implement exact Sections 3.3 order and conditional semantics |
| House/Super | missing House veteran/Side and Super AuxBuilding paths | implement exact member-major live reads |
| Anim | skipped entirely | implement fixed-Art live recursive loop |
| Techno/subclasses | broad subset only | implement inheritance, conditional weapon bank, and subclass tails in exact order |
| Weapon | family-batched Projectile/Warhead/ParticleSystem subset | member-major full Anim/PS/Warhead/Bullet order |
| Bullet | Airburst/Shrapnel subset | prepend fixed-Art `[Image] Trailer` |
| Warhead/Voxel | omitted | implement exact bodies and timing |
| post readers | partial General/Combat/Radiation/Special/Crate grouping | exact Crate/Combat/Radiation/Elevation/Wall/AudioVisual/Special/Tiberium order; remove spurious `ParaDropPlane`; add Special Warhead reread |
| lists | current generic list access trims tokens | preserve native whole-string trim but no per-token trim, empty collapse, repeats, and overlay retention |
| body timing | snapshots/batches lose live registry growth | use live indices and immediate factory calls |
| compatibility | final merged INI/RuleSet treated as convenient authority | keep unchanged downstream projection, but make trace discard explicit in compatibility-only loaders |

The allocated SuperWeaponType count carried with the receipt must come from the
native Type registry, including bodyless/parse-skipped entries. Parsed
`RuleSet.super_weapons.len()` is not equivalent and may not size the later
per-House Super constructor block.

The required fixed-Art transport also changes load ordering: current production
loads/consumes Rules before `load_art_ini`, then discards the partial trace. Exact
processing must have the fixed Art source available at the processor boundary
without contaminating the merged Rules INI or its hash.

## 8. Open Questions — Resolved Investigation Log

- `[RESOLVED] OQ-01` — Pre-type constructor stages are explicit registries,
  `[AI]`, and the 89 ordered `[General]` sites. All intervening readers were
  direct-plus-one-helper-depth audited and have no Type factory. (evidence:
  `0x00668BF0`, `0x00672AE0`, `0x0066D530`)
- `[RESOLVED] OQ-02` — Section 3.2 gives the exact explicit order and Side-key
  exception; Section 4 gives 24-byte storage, full-input lookup, repeat, Side
  sentinel, and failure semantics. Particle is non-ID-bearing. (evidence:
  `0x00410800`, `0x006A4550`, `0x006A46D0`, representative factories)
- `[RESOLVED] OQ-03` — House reads VeteranInfantry, VeteranUnits,
  VeteranAircraft, then Side; the first three target later loops and receive
  bodies in the same pass. (evidence: `0x00511850`, `0x00679A10`)
- `[RESOLVED] OQ-04` — SuperWeapon reads WeaponType then AuxBuilding; both
  target later loops. (evidence: `0x006CEA20`)
- `[RESOLVED] OQ-05` — The fixed-Art Anim closure is
  Next/Spawns/TiberiumSpawnType/Bounce/Expire/Trailer/Warhead/SpawnsParticle;
  live Anim growth is same-sweep and Particle costs no ID. (evidence:
  `0x00427D00`, `g_ArtINI @ 0x00887180`)
- `[RESOLVED] OQ-06` — Section 3.4 records the complete Techno inheritance,
  data-dependent weapon bank, Enslaves/Spawns helpers, and Building/Aircraft/
  Unit/Infantry tails. (evidence: `0x00712170`, subclass readers)
- `[RESOLVED] OQ-07` — Weapon is member-major:
  Anim-list/scalars, AttachedParticleSystem, Warhead, Projectile. (evidence:
  `0x00772080`)
- `[RESOLVED] OQ-08` — Bullet reads Rules Image, fixed-Art Trailer, then Rules
  AirburstWeapon and ShrapnelWeapon; its Anim/Weapon children wait for the next
  pass. (evidence: `0x0046BEE0`)
- `[RESOLVED] OQ-09` — Warhead, Particle, ParticleSystem, and VoxelAnim paths
  are enumerated in Section 3.4; Terrain, Smudge, and Overlay have no
  ID-bearing child factory. (evidence: `0x0075D3A0`, `0x00644F50`,
  `0x006442D0`, `0x0074B050`, cold decompiles)
- `[RESOLVED] OQ-10` — Weapon post is numeric-only and Building post returns;
  neither allocates. (evidence: `0x007729F0`, `0x00465CB0`)
- `[RESOLVED] OQ-11` — Section 3.5 records exact Crate, Combat, Radiation,
  Elevation, Wall, AudioVisual, SpecialWeapons plus Warhead reread, Tiberium
  Debris, and final CommandBar order. (evidence: reader entries in Section 3.5)
- `[RESOLVED] OQ-12` — All `ReadTypeData` family loops reload live count;
  Section 5 classifies every child as same-pass or later-pass. (evidence:
  `0x00679A10` loop back-edges and each reader's target family)
- `[RESOLVED] OQ-13` — Whole strings outer-trim, list tokens do not; missing
  values retain prior state, generic sentinels clear/skip, Side has no sentinel,
  repeats cost no constructor, and Rust may hard-error on OOM. (evidence:
  `ReadString`, `strtok`, factory decompiles, `0x004756F0`)
- `[RESOLVED] OQ-14` — Stock activation checkpoints and custom-only rows are in
  Section 6. TS-looking keys still allocate when the YR reader consumes present
  data; TS gameplay is not imported. (evidence: `ini/rulesmd.ini`,
  `ini/artmd.ini`, active binary call graph)
- `[RESOLVED] OQ-15` — `Load_Game_Rules @ 0x0052CD70` opens `ARTMD.INI` before
  the availability-gated `Process` call at `0x0052D317`, whose immediate
  argument is the stack-local `LANGRULE.INI`, not selected `RULESMD.INI`.
  Active stock has no LANGRULE and skips that call. Every later pass reads the
  same `g_ArtINI`; Rust must pass fixed Art independently of Rules layers.
  (evidence: `0x0052D00F..0x0052D317`,
  [LOAD_GAME_RULES_COLD_START_NATIVE_REGISTRY_PRESTATE_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/LOAD_GAME_RULES_COLD_START_NATIVE_REGISTRY_PRESTATE_REINVESTIGATION_GHIDRA_REPORT.md),
  `0x00679A10`)
- `[RESOLVED] OQ-16` — Section 7 gives the live Rust match/omit/misorder table.
- `[RESOLVED] OQ-17` — Final registry lengths cannot reconstruct member-major
  order, same/later-pass body activation, duplicate long-name constructors, or
  Special's Warhead reread. A merged INI also resurrects an earlier orphan body
  that native never read. (evidence: Sections 4-5)
- `[RESOLVED] OQ-18` — The first exhaustive traversal added `[AI]` tail sites,
  the final 26 General sites, Side's sentinel exception, full name truncation,
  AudioVisual, Special's Warhead reread, and Tiberium Debris. A cold second
  traversal of the top-level call order, every Type body, and every post reader
  was zero-add after those corrections. (evidence: coverage ledger below)
- `[RESOLVED] OQ-19` — Adversarial answers:
  (1) a repeated ordinary explicit name costs zero, but a >24-byte spelling may
  repeatedly construct because it cannot match its truncated stored ID;
  (2) a recursive Anim child receives its fixed-Art body in the same live sweep;
  (3) a Bullet-created Weapon spends immediately but waits until the next pass
  for its body;
  (4) a Warhead-created VoxelAnim receives a body later this pass while its Anim
  child waits, and Special's post-loop Warhead reread moves all those children
  behind the ordinary loops;
  (5) if a late-created type's body existed only in an earlier Rules pass, that
  body is never read merely because a later pass finally reaches the family.
- `[RESOLVED] OQ-20` — Certainty-gated annotation candidates are the true
  `ReadGeneral` entry `0x0066D530`, true full Warhead reader entry `0x0075D3A0`,
  `[AI]` Building-list reader `0x00672AE0`, Side scan `0x006A46D0`, and the final
  CommandBar classification of `FUN_00674650`. No Ghidra metadata was changed;
  synchronization was not requested for this implementation transaction.

## 9. Implementation handoff

### 9.1 Required ownership/API

1. Retain the non-Clone event and trace receipt. Rename `canonical_name` to
   `native_stored_id` or document that it is exactly the stored 24-byte ID.
2. Introduce one move-only `NativeRulesRegistryState` holding ordered stored IDs
   for all 17 processor families (the 16 ID families plus Particle) and the
   non-ID Tiberium slots. It must be seedable from the earlier pre-reset owner,
   not implicitly default-empty in production.
3. Make exact processing require `&IniFile` fixed Art. A compatibility-only API
   may omit trace production, but no API may label a Rules-only receipt exact.
4. Return the final allocated SuperWeaponType registry length with the receipt.
5. Move the state/receipt once through `RulesLayerStack -> ProcessedRulesLayers
   -> LoadedRules -> fresh scenario-prefix owner`; make intentional projection-
   only discard seams explicit in their names.
6. Keep fixed Art outside the compatibility Rules merge and Rules stack hash.
   It is a separate native input whose own source hash may be asserted by exact
   retail tests.

### 9.2 Required processor behavior

1. Apply every pass independently in the Section 3 order.
2. Read caller-sized strings before lookup; do not trim inside the factory.
3. Search an ordered stored-ID vector against the full incoming string;
   truncate only when constructing/storing/emitting.
4. Give Side its plain no-sentinel lookup. Give generic families native
   `none`/`<none>` handling.
5. Use native comma tokenization: no per-token trim, collapsed empty fields,
   left-to-right calls, null skip, repeated pointer retention.
6. Walk each `ReadTypeData` family with a live index. Do not precollect a family
   snapshot or family-batch child references.
7. Overlay the current object's effective body at the time native reads it,
   then evaluate the Techno weapon-bank gates from that live state.
8. Read Anim bodies, Aircraft `Trailer`, Building `ToOverlay`, and Bullet
   `Trailer` only from fixed Art. Do not project Art keys into Rules bodies.
9. Execute Special's seven-family interleave, then the Warhead reread, then
   Tiberium slot/body processing.
10. Reject negative Tiberium slots and allocation failures explicitly; preserve
    native sparse-high behavior, which creates exactly one new slot and does not
    fill gaps.

### 9.3 Acceptance tests

The narrow implementation suite must include:

- explicit source order, family repeats, Side membership exclusion, Side
  `none`, Particle zero-cost, 31-byte caller truncation, and repeated >24-byte
  stored-ID collision;
- all 20 `[AI]` and 89 General sites in exact order, absence of
  `ParaDropPlane`, and the stock five-event General checkpoint;
- House and Super member-major paths plus bodyless Super count;
- recursive fixed-Art Anim same-sweep growth, fixed-Art Aircraft Trailer, and
  fixed-Art Building ToOverlay;
- conditional Techno numbered versus ordinary weapon banks, including
  `EliteWeapon{i}`, Enslaves, and subclass tails;
- two-Weapon member-major ordering and full Anim/PS/Warhead/Bullet sequence;
- Bullet fixed-Art Trailer before late Airburst/Shrapnel weapons and a fixture
  proving their bodies wait for the next pass;
- Warhead/Particle/ParticleSystem/Voxel timing, including absent HoldsWhat's
  native default Particle behavior;
- exact Crate through CommandBar tail, two Smoke reads, Special family
  interleave plus Warhead reread, and Tiberium Debris;
- a pass-boundary orphan-body fixture proving a merged INI is not authority;
- `ProcessedRulesLayers -> LoadedRules` move preservation of event vector,
  registry state, Super count, compatibility INI, and source hash;
- a retail `RULESMD.INI` + fixed `ARTMD.INI` oracle checking the full event
  count/family/name hash plus load-bearing subsequences, not only final lengths.

## 10. Verification and coverage ledger

| Surface | Method | Result |
|---|---|---|
| top-level Process order | fresh decompile/disassembly and direct callee scan | verified; zero-add on second traversal |
| 15 explicit registry readers | caller and helper decompiles, retail section census | verified including Side key and Particle exclusion |
| pre-type zero-event readers | direct plus one-helper-depth factory scan | verified zero |
| `[AI]` | direct 15 sites plus helper-backed final 5 | verified 20, exact order |
| `[General]` | all direct/helper factory call sites | verified 89, exact family/shape/order |
| `ReadTypeData` owner | every loop header/back-edge and vslot target | verified live counts and family order |
| House/Super | complete reader decompile | verified |
| fixed-Art Anim | complete reader decompile plus stock Art scan | verified |
| Techno inheritance | full constructor-capable call-site scan; conditional cold spot | verified, including Enslaves then Spawns |
| Building/Aircraft/Unit/Infantry tails | complete subclass decompiles; Infantry string-address cold check | verified; DeadBodies precedes DeathAnims |
| Weapon/Bullet/Warhead | complete reader decompiles; fixed-Art branch | verified member-major and late-body timing |
| remaining type readers | complete Terrain/Smudge/Overlay/Particle/PS/Voxel/Mission scan | verified; no hidden ID path |
| Weapon/Building post | direct decompile | verified zero |
| post readers | complete owner/callee traversal; Special/Tiberium cold spot | verified through final CommandBar |
| string/list semantics | constructor, resolver, ReadString, and token-loop decompiles | verified including Side exception and 24-byte collision |
| stock activation | ordered retail `rulesmd.ini`/`artmd.ini` scans | verified checkpoints; algorithm remains production authority |
| current Rust | ownership/use census and full processor diff read | verified mismatch table |
| OpenTS correspondence | `C:/Users/enok/Documents/OpenTS/code/rules.cpp` read-only navigation | lead only; TS `HSBuilding`, Firestorm, and AudioVisual/Special order were rejected unless active binary independently proved them |

### Exhaustion gate

- **Zero-add pass:** passed after the corrections listed in OQ-18.
- **Five adversarial questions:** answered in OQ-19.
- **Cold spot 1:** the data-dependent Techno weapon bank and the final General
  helper cluster were retraced independently; no extra factory remains.
- **Cold spot 2:** Special's Warhead reread, Tiberium row semantics/Debris, and
  final CommandBar were retraced independently; no extra ID-bearing path remains.
- **Open questions:** zero.
- **Implementation status:** research complete; Rust parity remains open until
  every Section 9 acceptance test passes and the receipt is consumed by the
  shared fresh-load prefix owner.

## Sources

- Active `gamemd.exe` in the connected live Ghidra project:
  `RulesClass::Process @ 0x00668BF0`, `ReadTypeData @ 0x00679A10`, and every
  constructor/reader/helper address cited above.
- Active startup topology: `Load_Game_Rules @ 0x0052CD70`, optional
  stack-local LANGRULE Process call `0x0052D317`, fixed
  `g_ArtINI @ 0x00887180`, then the caller's live Anim and Building sweeps;
  see [LOAD_GAME_RULES_COLD_START_NATIVE_REGISTRY_PRESTATE_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/LOAD_GAME_RULES_COLD_START_NATIVE_REGISTRY_PRESTATE_REINVESTIGATION_GHIDRA_REPORT.md).
- Native name owner: `AbstractTypeClass::Constructor @ 0x00410800`;
  `AbstractClass::AssignUniqueID @ 0x00410230`; `SideClass::Constructor @
  0x006A4550`; Side scan `0x006A46D0`.
- Retail data: `ini/rulesmd.ini`, `ini/artmd.ini`; `ini/rules.ini` and
  `ini/art.ini` used only for negative active-root comparison.
- Prior binary-backed context:
  `docs/research/bridges/01-assets-map-load-overlay/FULL_INIT_AND_PREVIEW_NATIVE_ID_PREFIX_REINVESTIGATION_GHIDRA_REPORT.md`,
  [docs/research/RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md),
  [docs/research/ANIM_CLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ANIM_CLASS_GHIDRA_REPORT.md), and
  [docs/research/BULLETTYPECLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLETTYPECLASS_GHIDRA_REPORT.md).
- Current Rust ownership and processor:
  `src/rules/ini_parser.rs`, `src/rules/ini_parser_tests.rs`,
  `src/app/loading/init_helpers.rs`, `src/app/loading/init.rs`, and
  `src/headless_scenario.rs`.
- Secondary navigation lead only:
  `C:/Users/enok/Documents/OpenTS/code/rules.cpp`. Every material conclusion
  taken forward was independently checked against active `gamemd.exe` and YR
  retail data; TS-only correspondences were excluded.
