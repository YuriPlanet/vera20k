# TechnoTypeClass Base — Ghidra Research Report

**Primary Address:** `0x00710AF0` (ctor) / `0x00712170` (ReadINI) / vtable `0x007F4ED8`
**Inheritance Chain:** `AbstractClass → AbstractTypeClass → ObjectTypeClass → TechnoTypeClass → {Unit|Infantry|Building|Aircraft}TypeClass`
**Base Instance Size:** **0xDF8 bytes** (subclass fields begin at byte 0xDF8 / index `[0x37E]`)
**Confidence:** High — every claim traced in live Ghidra decompilation. TS-legacy keys verified against binary string table.
**Active in YR:** Yes — base type-class parsed at rules load for every `[UnitTypes]` / `[InfantryTypes]` / `[BuildingTypes]` / `[AircraftTypes]` entry. Individual fields carry per-field verdicts in §10.

This is the canonical standalone reference for the TechnoTypeClass base. Any
downstream subclass doc (UnitTypeClass, InfantryTypeClass, BuildingTypeClass,
AircraftTypeClass) should cite this document for inherited fields rather than
re-discover them.

---

## 1. Overview

TechnoTypeClass is the base type-class template for every "techno" object in
Yuri's Revenge — anything that has an owner, can be built, can fire a weapon,
or participates in production/victory conditions. At rules-load time, every
entry in `[UnitTypes]`, `[InfantryTypes]`, `[BuildingTypes]`, and
`[AircraftTypes]` instantiates a derived TypeClass whose first `0xDF8` bytes
are the inherited base template.

The base owns Cost, Speed, Armor (via parent), Strength (via parent),
Prerequisite, Owner bitmask, all Voice*/Sound* SFX IDs, VeteranAbilities,
weapon slots (Primary/Secondary/Elite variants), cameo references,
cloaking/deployment/bunker flags, and ~300 other per-type settings. It is the
largest ReadINI in gamemd.exe at ~332 distinct `Read_*` calls across ~2000
decompiled lines.

---

## 2. Class Hierarchy

The *type-class* hierarchy is parallel to — but structurally separate from —
the *instance* hierarchy documented in [ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md) /
[OBJECTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_GHIDRA_REPORT.md) / [TECHNOCLASS_STRUCT_LAYOUT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNOCLASS_STRUCT_LAYOUT.md). Every
live techno object (an actual unit/building on the map) points to one of
these type-class templates via its `TechnoClass + 0x14C` field.

```
AbstractClass (root, ~0x10 bytes)
   │  virtuals: QueryInterface / AddRef / Release / IsDirty / Load / Save / GetSizeMax / ...
   │
   └─▶ AbstractTypeClass          vtable 0x007E2000   bytes [0x00 .. 0x94]
          │  ctor    0x00410800   super → AbstractClass::Constructor_Full
          │  ReadINI 0x00410A60   reads Name, UIName
          │
          └─▶ ObjectTypeClass     vtable 0x007EF2D8   bytes [0x98 .. 0x293]
                 │  ctor    0x005F7090   super → AbstractTypeClass::ctor
                 │  ReadINI 0x005F92D0   reads 24 keys incl. Image, Armor, Strength, Immune
                 │
                 └─▶ TechnoTypeClass   vtable 0x007F4ED8   bytes [0x294 .. 0xDF7]
                        │  ctor    0x00710AF0    super → ObjectTypeClass::ctor
                        │  ReadINI 0x00712170    reads ~332 keys
                        │
                        ├─▶ UnitTypeClass       vtable 0x007F6218   ctor 0x007470D0   total ~0xE80
                        ├─▶ InfantryTypeClass   vtable 0x007EB610   ctor 0x005236A0   total ~0xED0
                        ├─▶ BuildingTypeClass   vtable 0x007E4570   ctor 0x0045DD90   total ~0x1794
                        └─▶ AircraftTypeClass   vtable 0x007E2868   ctor 0x0041C8B0   total ~0xE10
```

**Every subclass writes `[0x37E]` (byte 0xDF8) as its first new field** after
the super-call returns. This is the definitive boundary between base and
subclass ranges.

Each type-class vtable carries **4 vtable slots at bytes 0x00 / 0x04 / 0x08 /
0x0C** (multi-inheritance / COM `IPersistStream`-style thunks). Every level
overrides all four in its own ctor.

---

## 3. Instance Layout Bands

| Band | Byte Range | Size | Owner | Evidence |
|------|-----------|------|-------|----------|
| AbstractClass header (vtables + misc) | `[0x00 .. 0x23]` | 0x24 | AbstractClass | inherited; `AbstractTypeClass::ctor` overrides vtables at 0x00/0x04/0x08/0x0C |
| `CCINIClass*` pointer | `[0x24]` | 4 | AbstractTypeClass | zeroed in ctor; loaded during ReadINI |
| `UIName` buffer | `[0x3D .. 0x5C]` | 0x20 | AbstractTypeClass | 32-byte inline char array |
| CSF-resolved UIName pointer | `[0x60]` | 4 | AbstractTypeClass | defaults to `&DAT_00887734` (empty-string sentinel) |
| `Name` / ID buffer | `[0x64 .. 0x94]` | 0x31 | AbstractTypeClass | 49-byte inline char array; `strncpy` from ctor arg 2 |
| **ObjectTypeClass portion** | `[0x98 .. 0x293]` | 0x1FC | ObjectType | Armor, Strength, Image, AlphaImage, Crushable, Bombable, etc. |
| **TechnoTypeClass portion** | `[0x294 .. 0xDF7]` | 0xB64 | TechnoType | All remaining base fields |
| Subclass portion | `[0xDF8 .. N]` | varies | Unit/Inf/Bldg/Air | Begins at index `[0x37E]` |

**The base instance is 0xDF8 bytes (3576 decimal).** The previous "~0xDF4"
estimate was close but slightly off; the `[0x37D]` DWORD at byte 0xDF4 is
reserved/unused but counts toward the size, and subclass ctors all begin at
`[0x37E]` = byte 0xDF8.

### 3.1 Why 4 vtables at the head

Westwood's type-class hierarchy uses MSVC multiple-inheritance / COM interface
thunks. Each type-class carries **four vtable pointers** at the head of the
struct (offsets 0/4/8/0xC). The primary vtable provides the full set of
virtual methods; the three secondaries are RTTI + interface-routing thunks.
Leaf ctors (e.g., `BuildingTypeClass::ctor`) re-overwrite all four with their
own addresses after the super-call chain completes.

| Level | Primary vtable | Evidence |
|-------|----------------|----------|
| AbstractTypeClass | `0x007E2000` | verified via vtable label + ctor write |
| ObjectTypeClass | `0x007EF2D8` | verified via vtable label + ctor write |
| TechnoTypeClass | `0x007F4ED8` | verified via vtable label + ctor write |
| UnitTypeClass | `0x007F6218` | verified |
| InfantryTypeClass | `0x007EB610` | verified |
| BuildingTypeClass | `0x007E4570` | verified |
| AircraftTypeClass | `0x007E2868` | verified |

---

## 4. Vtable Layout — TechnoTypeClass @ `0x007F4ED8`

Decoded from `read_memory(0x007F4ED8, 0x80)`:

| Slot | Byte | Target Address | Identity | Inherited / Override |
|------|------|----------------|----------|----------------------|
| 0 | 0x00 | 0x00410260 | `AbstractClass::QueryInterface` (IUnknown) | inherited |
| 1 | 0x04 | 0x00410300 | `AbstractClass::AddRef` | inherited |
| 2 | 0x08 | 0x00410310 | `AbstractClass::Release` | inherited |
| 3 | 0x0C | 0x004C9150 | `Stub__ReturnZero` (xor eax,eax; ret) | shared stub |
| 4 | 0x10 | 0x00410450 | `AbstractClass::IsDirty` | inherited |
| 5 | 0x14 | **0x007162F0** | **`TechnoTypeClass::Load`** (IStream) | **override** |
| 6 | 0x18 | **0x00716DC0** | **`TechnoTypeClass::Save`** (IStream) | **override** |
| 7 | 0x1C | **0x007170A0** | **`TechnoTypeClass::GetSizeMax`** | **override** |
| 8 | 0x20 | 0x007179A0 | `TechnoTypeClass::~TechnoTypeClass` (scalar-deleting dtor) | override |
| 9 | 0x24 | 0x00410470 | `RET` thunk (no-op void) | inherited stub |
| 10 | 0x28 | 0x00410480 | `RET 8` thunk (two-arg no-op setter) | inherited stub |
| 11 | 0x2C | 0x004C9150 | `Stub__ReturnZero` (RTTI/category fallback) | shared stub |
| 12 | 0x30 | 0x004C9150 | `Stub__ReturnZero` | shared stub |
| 13 | 0x34 | **0x007171A0** | **`TechnoTypeClass::Compute_CRC`** | **override** |
| 14 | 0x38 | 0x00410490 | small thunk | inherited |
| 15 | 0x3C | 0x004104A0 | `RET 4` thunk | inherited |
| 16 | 0x40 | 0x004104B0 | `RET 4` thunk | inherited |
| 17 | 0x44 | 0x00410440 | tiny thunk | inherited |
| 18 | 0x48 | 0x004104C0 | `AbstractClass::GetCoords` (placeholder for TypeClass) | inherited |
| 19 | 0x4C | 0x004104F0 | small method | inherited |
| 20 | 0x50 | 0x00410520 | tiny thunk | inherited |
| 21 | 0x54 | 0x00410530 | tiny thunk | inherited |
| 22 | 0x58 | 0x00410540 | small method | inherited |
| 23 | 0x5C | 0x00410570 | `RET` thunk | inherited |
| 24 | 0x60 | 0x00410C20 | `RET 4` empty-getter stub | inherited |
| 25 | 0x64 | **0x00712170** | **`TechnoTypeClass::Read_INI`** | **override** |
| 26 | 0x68 | 0x00410B90 | small method (~70 bytes) | inherited |
| 27 | 0x6C | 0x0041CF80 | 20-byte copy-out thunk (likely Coord getter) | inherited |
| 28 | 0x70 | **0x00711EC0** | **`TechnoTypeClass::Cost_Of`** (reads flag `+0xC99`; INT_MAX if set else `+0x6CC`) | **override** |
| 29 | 0x74 | 0x00716290 | (GetPipMax slot — subclass overrides) | inherited |
| 30 | 0x78 | 0x005F75C0 | ObjectTypeClass 3-DWORD getter | inherited from ObjectType |
| 31 | 0x7C | 0x005F75E0 | ObjectTypeClass helper | inherited from ObjectType |

### 4.1 Non-vtable TechnoTypeClass methods

| Address | Name | Purpose |
|---------|------|---------|
| `0x00710AF0` | `TechnoTypeClass::TechnoTypeClass` | Primary ctor; args: `(this, id_string, kind_index)`. Super-calls ObjectType ctor. |
| `0x00711840` | `TechnoTypeClass::TechnoTypeClass` | Overload (copy-style) |
| `0x00711AE0` | `TechnoTypeClass::TechnoTypeClass` | Overload |
| `0x00711EE0` | `TechnoTypeClass::GetBuildTime` | Formula: `ftol(Cost × Rules.BuildSpeed × 0.9)` |
| `0x00717800` | `TechnoTypeClass::GetFlightLevel` | Returns `+0x618` (`FlightLevel`); if -1 → `Rules[0x7b4]` | (corrected 2026-05-28: was `GetSpeed`; Ghidra label is `TechnoTypeClass__GetFlightLevel` — `read_memory 0x007F4ED8` slot 29 + `decompile_function 0x00717800` — ROOT_CAUSE: RTTI_LABEL_DRIFT) |
| `0x00716290` | `TechnoTypeClass::GetPipMax` | Inherited by subclasses; overridden in UnitType for harvesters |
| `0x00712170` | `TechnoTypeClass::Read_INI` | The 332-read workhorse |

---

## 5. Field Layout

Tables list byte offsets from `TechnoTypeClass *`. Types: **b** = byte, **w** =
word, **i** = int32, **u** = uint32, **f** = float32, **d** = double,
**char[N]** = inline string buffer, **vec** = inline DynamicVectorClass
subobject (0x1C bytes).

### 5.1 AbstractClass header (`[0x00 .. 0x23]`) — inherited

4 vtable slots at 0x00/0x04/0x08/0x0C plus ~0x14 bytes of AbstractClass
instance state (flags, ID, registration). Not extended in this report — see
[ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md).

### 5.2 AbstractTypeClass portion (`[0x24 .. 0x94]`)

| Offset | Size | Type | Field | Notes |
|--------|------|------|-------|-------|
| 0x24 | 4 | `CCINIClass*` | `IniHandle` | Set during ReadINI; zeroed in ctor |
| 0x3D | 0x20 | char[32] | `UIName` (raw key) | Read from INI `UIName=` |
| 0x60 | 4 | `wchar_t*` | `UIName` (CSF-resolved) | `StringTable::LoadString(UIName, 0xD7)` or `&DAT_00887734` |
| 0x64 | 0x31 | char[49] | `Name` / ID | `strncpy` from ctor arg 2; read from INI `Name=` (overrides ctor default) |

### 5.3 ObjectTypeClass portion (`[0x98 .. 0x293]`)

| Offset | Size | Type | Field | INI Key | Default | Notes |
|--------|------|------|-------|---------|---------|-------|
| 0x98 | 3 | RGB24 | `RadialColor.R/G/B` | `RadialColor` | 0,0,0 | Read via ReadColorRGB |
| 0x9C | 4 | int | `Armor` | `Armor` | 0 (`none`) | enum; see §6.1 |
| 0xA0 | 4 | int | `Strength` | `Strength` | 0 | HP |
| 0xA4 | 4 | `SHPStruct*` | `Image SHP` | — | — | **Runtime-derived**; reloaded from MIX at Load time |
| 0xC8..0x1E7 | 288 | int[72] | (TS-era anim tables, zeroed) | — | 0 | Likely per-frame/per-facing anim descriptors; not parsed from INI |
| 0x1E8 | 1 | bool | `NoSpawnAlt` | `NoSpawnAlt` | false | |
| 0x1F0 | 4 | int | `CrushSound` | `CrushSound` | -1 | VOC index via `VocClass::FindByName` |
| 0x1F4 | 4 | int | `AmbientSound` | `AmbientSound` | -1 | VOC index |
| 0x1F8 | 0x19 | char[25] | `Image` | `Image` | (self ID) | Art section reference |
| 0x211 | 1 | bool | `AlternateArcticArt` | `AlternateArcticArt` | false | TS-era theater variant |
| 0x213 | 0x19 | char[25] | `AlphaImage` | `AlphaImage` | `""` | Alpha sprite overlay |
| 0x22C | 1 | bool | `Theater` | `Theater` | false | Theater-specific art |
| 0x22D | 1 | bool | `Crushable` | `Crushable` | false | |
| 0x22E | 1 | bool | `Bombable` | `Bombable` | **true** | Can be bombed |
| 0x22F | 1 | bool | `RadarInvisible` | `RadarInvisible` | false | |
| 0x230 | 1 | bool | `Selectable` | `Selectable` | **true** | |
| 0x231 | 1 | bool | `LegalTarget` | `LegalTarget` | **true** | |
| 0x232 | 1 | bool | `Insignificant` | `Insignificant` | false | |
| 0x233 | 1 | bool | `Immune` | `Immune` | false | **LIVE in YR** — damage immunity |
| 0x236 | 1 | bool | `Voxel` | `Voxel` | false | Uses voxel art |
| 0x237 | 1 | bool | `NewTheater` | `NewTheater` | false | |
| 0x238 | 1 | bool | `HasRadialIndicator` | `HasRadialIndicator` | false | |
| 0x239 | 1 | bool | `IgnoresFirestorm` | `IgnoresFirestorm` | false | TS-era — firestorm off in YR |
| 0x23A | 1 | bool | `UseLineTrail` | `UseLineTrail` | false | |
| 0x23B..0x23D | 3 | RGB24 | `LineTrailColor.R/G/B` | `LineTrailColor` | 0x80,0x80,0x80 | |
| 0x240 | 4 | int | `LineTrailColorDecrement` | `LineTrailColorDecrement` | 0x10 (16) | |

Tail block `[0x244 .. 0x290]` is zeroed and not touched by `ObjectTypeClass::Read_INI`
— filled by runtime systems (anim caches, theater handles).

### 5.4 TechnoTypeClass portion (`[0x294 .. 0xDF7]`)

This is the main deliverable. 332 distinct Read_* calls map to these fields.
Columns: `#` is the read-order index; `Default` is the prior/ctor value (YR
base). Per-field YR activity verdicts are in §10.

Where multiple reads write into the same region (loops over `Weapon%d`,
`Stage%d`, `AlternateFLH%d`), the first row represents the base entry and a
collapsed "+28*n" row represents the loop stride.

**Timing & locomotion**:

| Offset | Size | Field | INI Key | Default | Notes |
|--------|------|-------|---------|---------|-------|
| 0x294 | 4 | int | `WalkRate` | 1 (ctor) | |
| 0x298 | 4 | int | `IdleRate` | 0 | |
| 0x29C..0x2AD | 18 | byte[18] | `VeteranAbilities` | all 0 | **18-byte flag array** (NOT 96-bit bitmask); see §6.4 |
| 0x2C0 | 8 | double | `SpecialThreatValue` | 0 | |
| 0x2C8 | 8 | double | `MyEffectivenessCoefficient` | `Rules+0x1040` if INI 0 | |
| 0x2D0 | 8 | double | `TargetEffectivenessCoefficient` | `Rules+0x1048` if INI 0 | |
| 0x2D8 | 8 | double | `TargetSpecialThreatCoefficient` | `Rules+0x1050` if INI 0 | |
| 0x2E0 | 8 | double | `TargetStrengthCoefficient` | `Rules+0x1058` if INI 0 | |
| 0x2E8 | 8 | double | `TargetDistanceCoefficient` | `Rules+0x1060` if INI 0 | |
| 0x2F0 | 8 | double | `ThreatAvoidanceCoefficient` | 0 | |
| 0x2F8 | 4 | int | `SlowdownDistance` | 500 | |
| 0x300 | 8 | double | `DeaccelerationFactor` | ~0.002 | `0x3F60624D D2F1A9FC` |
| 0x308 | 8 | double | `AccelerationFactor` | ~0.030 | `0x3F9EB851 EB851EB8` |
| 0x310 | 4 | int | `CloakingSpeed` | 7 | |
| 0x314 | 0x1C | vec\<VoxelAnimType\*\> | `DebrisTypes` | empty | vtable PTR_FUN_007F0D3C |
| 0x330 | 0x1C | vec | (unnamed) | empty | vtable PTR_FUN_007E4DD8 |
| 0x330..0x348 | difficulty int[3] | `DebrisMaximums` | 0 | Via DifficultyClass::ReadINI_IntVector |
| 0x34C | 16 | CLSID | `Locomotor` | TeleportLocomotion (default) | ReadCLSID |
| 0x360 | 8 | double | (voxel-hotspot float; runtime-derived) | 0 | Set post-read from voxel frame data |
| 0x368 | 8 | double | (voxel-hotspot float; runtime-derived) | 1.0 | |
| 0x370 | 8 | double | `Weight` | 2.0 | |
| 0x378 | 8 | double | `PhysicalSize` | 1.0 | |
| 0x380 | 8 | double | **`Size`** | 0 | (key resolved: `@DAT_00820178`) |
| 0x388 | 8 | double | `SizeLimit` | 0 | |
| 0x390 | 1 | bool | `HoverAttack` | false | |
| 0x394 | 4 | int | `VHPScan` | 0 | See §6.6 |
| 0x398 | 4 | int | (UnitType patches: PipScale default for harvesters) | 15 | |
| 0x3A0 | 8 | double | `RollAngle` | ~0.524 rad | -1 sentinel: if not -1, stores `val*DEG_TO_RAD` |
| 0x3A8 | 8 | double | `PitchSpeed` | 0 | |
| 0x3B0 | 8 | double | `PitchAngle` | ~0.349 rad | Same sentinel behavior |
| 0x3B8 | 4 | int | `BuildLimit` | INT_MAX | |
| 0x3BC | 4 | int | `Category` | -1 | enum; see §6.5 |
| 0x3C8 | 8 | double | `DeployTime` | 0 | |
| 0x3D0 | 4 | int | `FireAngle` | 8 | |
| 0x3D4 | 4 | int | `PipScale` | 0 | enum; see §6.3 |
| 0x3D8 | 1 | bool | `PipsDrawForAll` | false | |
| 0x3DC | 4 | int | `LeptonMindControlOffset` | 70 | |
| 0x3E0 | 4 | int | `PixelSelectionBracketDelta` | 0 | |
| 0x3E4 | 4 | int | `PipWrap` | 0 | |

**Owner & prerequisites**:

| Offset | Size | Field | INI Key | Default | Notes |
|--------|------|-------|---------|---------|-------|
| 0x3E8 | 0x1C | vec | (Owner list container) | empty | vtable PTR_FUN_007ED90C |
| 0x3E8..0x400 | vec\<HouseType\*\> | `Owner` (list form) | — | empty | Stored tokenized list |
| 0x404 | 4 | `BuildingType*` | `DeploysInto` | nullptr | |
| 0x408 | 4 | `UnitType*` | `UndeploysInto` | nullptr | |
| 0x40C | 4 | `UnitType*` | `PowersUnit` | nullptr | |
| 0x410 | 1 | bool | `PoweredUnit` | false | |
| 0x414..0x528 | 10× 0x1C | vec[10] | (10 parallel per-house vectors) | empty | Per-house/per-facing state |
| 0x414..0x42C | vec | `VoiceSelect` | — | | ReadSoundList |
| 0x430..0x448 | vec | `VoiceSelectEnslaved` | — | | |
| 0x44C..0x464 | vec | `VoiceSelectDeactivated` | — | | |
| 0x468..0x480 | vec | `VoiceMove` | — | | |
| 0x484..0x49C | vec | `VoiceAttack` | — | | |
| 0x4A0..0x4B8 | vec | `VoiceSpecialAttack` | — | | |
| 0x4BC..0x4D4 | vec | `VoiceDie` | — | | |
| 0x4D8..0x4F0 | vec | `VoiceFeedback` | — | | |
| 0x4F4..0x50C | vec | `MoveSound` | — | | |
| 0x510..0x528 | vec | `DieSound` | — | | |

**Per-sound VOC-index fields** (`CCINIClass::ReadString → VocClass::FindByName`, -1 default):

| Offset | Field | INI Key |
|--------|-------|---------|
| 0x52C | `AuxSound1` | `AuxSound1` |
| 0x530 | `AuxSound2` | `AuxSound2` |
| 0x534 | `CreateSound` | `CreateSound` |
| 0x538 | `DamageSound` | `DamageSound` |
| 0x53C | `ImpactWaterSound` | `ImpactWaterSound` |
| 0x540 | `ImpactLandSound` | `ImpactLandSound` |
| 0x544 | `CrashingSound` | `CrashingSound` |
| 0x548 | `SinkingSound` | `SinkingSound` |
| 0x54C | `VoiceFalling` | `VoiceFalling` |
| 0x550 | `VoiceCrashing` | `VoiceCrashing` |
| 0x554 | `VoiceSinking` | `VoiceSinking` |
| 0x558 | `VoiceEnter` | `VoiceEnter` |
| 0x55C | `VoiceCapture` | `VoiceCapture` |
| 0x560 | `TurretRotateSound` | `TurretRotateSound` |
| 0x564 | `EnterTransportSound` | `EnterTransportSound` |
| 0x568 | `LeaveTransportSound` | `LeaveTransportSound` |
| 0x56C | `DeploySound` | `DeploySound` |
| 0x570 | `UndeploySound` | `UndeploySound` |
| 0x574 | `ChronoInSound` | `ChronoInSound` |
| 0x578 | `ChronoOutSound` | `ChronoOutSound` |
| 0x57C | `VoiceHarvest` | `VoiceHarvest` |
| 0x580 | `VoicePrimaryWeaponAttack` | `VoicePrimaryWeaponAttack` |
| 0x584 | `VoicePrimaryEliteWeaponAttack` | `VoicePrimaryEliteWeaponAttack` |
| 0x588 | `VoiceSecondaryWeaponAttack` | `VoiceSecondaryWeaponAttack` |
| 0x58C | `VoiceSecondaryEliteWeaponAttack` | `VoiceSecondaryEliteWeaponAttack` |
| 0x590 | `VoiceDeploy` | `VoiceDeploy` |
| 0x594 | `VoiceUndeploy` | `VoiceUndeploy` |
| 0x598 | `EnterGrinderSound` | `EnterGrinderSound` |
| 0x59C | `LeaveGrinderSound` | `LeaveGrinderSound` |
| 0x5A0 | `EnterBioReactorSound` | `EnterBioReactorSound` |
| 0x5A4 | `LeaveBioReactorSound` | `LeaveBioReactorSound` |
| 0x5A8 | `ActivateSound` | `ActivateSound` |
| 0x5AC | `DeactivateSound` | `DeactivateSound` |
| 0x5B0 | `MindClearedSound` | `MindClearedSound` |

**Core gameplay stats**:

| Offset | Size | Field | INI Key | Default | Notes |
|--------|------|-------|---------|---------|-------|
| 0x5B4 | 4 | int (enum) | `MovementZone` | 0 | Post-read: byte `0xD2C = (zone == 6)` |
| 0x5B8 | 4 | int (leptons) | `GuardRange` | 0 | ReadRange |
| 0x5BC | 4 | int | `MaxDebris` | 0 | |
| 0x5C0 | 4 | int | `MinDebris` | 0 | Post-clamp: `MinDebris ≥ 0`, `MaxDebris ≥ MinDebris` |
| 0x5C4 | 0x1C | vec | (unnamed) | empty | vtable PTR_FUN_007EB6F4 |
| 0x5D4 | 0x1C | vec\<AnimType\*\> | `DebrisAnims` | empty | Tokenized list |
| 0x5E0 | 4 | int | `Passengers` | 0 | |
| 0x5E4 | 1 | bool | `OpenTopped` | false | |
| 0x5E8 | 4 | int | `Sight` | 0 | |
| 0x5EC | 1 | bool | `ResourceGatherer` | false | |
| 0x5ED | 1 | bool | `ResourceDestination` | false | |
| 0x5EE | 1 | bool | `RevealToAll` | false | |
| 0x5EF | 1 | bool | `Drainable` | false | |
| 0x5F0 | 4 | int | `SensorsSight` | 0 | |
| 0x5F4 | 4 | int | `DetectDisguiseRange` | 0 | |
| 0x5F8 | 4 | int | `BombSight` | 0 | |
| 0x5FC | 4 | int | `LeadershipRating` | 5 | |
| 0x600 | 4 | int | `NavalTargeting` | 0 | |
| 0x604 | 4 | int | `LandTargeting` | 0 | |
| 0x608 | 4 | float | `BuildTimeMultiplier` | 1.0 | Stored as float cast of double read |
| 0x60C | 4 | int | `MindControlRingOffset` | 140 | |
| 0x610 | 4 | int | **`Cost`** | 0 | (key resolved: `@DAT_00825470`) — **the Cost field** |
| 0x614 | 4 | int | `Soylent` | 0 | Refund amount |
| 0x618 | 4 | int | `FlightLevel` | -1 | Read by `TechnoTypeClass::GetFlightLevel` (`0x00717800`); if -1, returns `Rules+0x7B4` default altitude (corrected 2026-05-28: cross-ref was `GetSpeed`; correct name is `GetFlightLevel` — ROOT_CAUSE: RTTI_LABEL_DRIFT) |
| 0x61C | 4 | int | `AirstrikeTeam` | 0 | |
| 0x620 | 4 | int | `EliteAirstrikeTeam` | 0 | |
| 0x624 | 4 | `TeamType*` | `AirstrikeTeamType` | nullptr | |
| 0x628 | 4 | `TeamType*` | `EliteAirstrikeTeamType` | nullptr | |
| 0x62C | 4 | int | `AirstrikeRechargeTime` | 0 | |
| 0x630 | 4 | int | `EliteAirstrikeRechargeTime` | 0 | |
| 0x634 | 4 | int | `TechLevel` | 255 | |
| 0x638 | 0x1C | vec\<int\> | `Prerequisite` | empty | IDs: positive=BuildingType idx, negative=keyword (see §6.2) |
| 0x654 | 0x1C | vec\<int\> | `PrerequisiteOverride` | empty | Same format |
| 0x670 | 4 | int | `ThreatPosed` | 0 | |
| 0x674 | 4 | int | `Points` (cache) | 0 | Copy of 0x728 |
| 0x678 | 4 | int | `Crushability`(?) | 0 | Clamped [0,100], then `(val*256)/100` clamped to 0xFF |
| 0x67C | 4 | int | (kind-index; UnitType patches: SpeedType default) | ctor arg 3 | |
| 0x680 | 4 | int | `InitialAmmo` | -1 | |
| 0x684 | 4 | int | **`Ammo`** | -1 | (key resolved: `@DAT_0081bbe0`) |
| 0x688 | 4 | int | `IFVMode` | 0 | |
| 0x68C | 4 | int (leptons) | `AirRangeBonus` | 0 | |
| 0x690..0x695 | 6×1 | bool | `BerserkFriendly`, `SprayAttack`, `Pushy`, `Natural`, `Unnatural`, `CloseRange` | 0 | |
| 0x698 | 4 | int | `Reload` | 0 | |
| 0x69C | 4 | int | `EmptyReload` | -1 | |
| 0x6A0 | 4 | int | `ReloadIncrement` | 0 | |
| 0x6A4 | 4 | int | `RadialFireSegments` | 0 | |
| 0x6A8 | 4 | int | `DeployFireWeapon` | 1 | |
| 0x6AC | 1 | bool | `DeployFire` | false | |
| 0x6AD | 1 | bool | `DeployToLand` | false | |
| 0x6AE | 1 | bool | `MobileFire` | **true** | |
| 0x6AF | 1 | bool | `OpportunityFire` | false | |
| 0x6B0 | 1 | bool | `DistributedFire` | false | |
| 0x6B1 | 1 | bool | `DamageReducesReadiness` | false | |
| 0x6B4 | 4 | float | `ReadinessReductionMultiplier` | 0 | |
| 0x6B8 | 4 | `UnitType*` | `UnloadingClass` | nullptr | |
| 0x6BC | 4 | `AnimType*` | `DeployingAnim` | nullptr | |
| 0x6C0 | 1 | bool | `AttackFriendlies` | false | |
| 0x6C1 | 1 | bool | `AttackCursorOnFriendlies` | false | |
| 0x6C4 | 4 | int | `UndeployDelay` | -1 | |
| 0x6C8 | 1 | bool | `PreventAttackMove` | false | |
| 0x6CC | 4 | uint | `Owner` (bitmask form) | 0 | See §6.7 — DATA-DRIVEN mapping |
| 0x6D0 | 4 | int | `AIBasePlanningSide` | -1 | |
| 0x6D4 | 1 | bool | `StupidHunt` | false | |
| 0x6D5 | 1 | bool | `AllowedToStartInMultiplayer` | **true** | |
| 0x6D6 | 24 | char[24] | (inline string buf) | copy of DAT_00889F64 | Default placeholder; filled by Read_INI |
| 0x6F0 | 4 | `SHPStruct*` | `Cameo` → SHP | nullptr | **Runtime-derived**; reloaded from INI+MIX each Load |
| 0x6F5 | 24 | char[24] | (inline string buf for AltCameo) | copy of DAT_00889F64 | |
| 0x710 | 4 | `SHPStruct*` | `AltCameo` → SHP | nullptr | **Runtime-derived**; reloaded from INI+MIX each Load |
| 0x718 | 4 | int | `RotCount` | 0 | |
| 0x71C | 4 | int | **`ROT`** | 0 | (key resolved: `@DAT_0081b164`) — Rate of Turn |
| 0x720 | 4 | int | `TurretOffset` | 0 | |
| 0x724 | 1 | bool | `CanBeHidden` | **true** | |
| 0x728 | 4 | int | `Points` | 0 | Also cached at 0x674 |
| 0x72C | 0x1C | vec\<AnimType\*\> | `Explosion` | empty | Tokenized |
| 0x748 | 0x1C | vec\<AnimType\*\> | `DestroyAnim` | empty | Tokenized |
| 0x764 | 4 | `ParticleSystemType*` | `NaturalParticleSystem` | nullptr | |
| 0x768..0x770 | 3×4 | int[3] | `NaturalParticleLocation` | 0,0,0 | 3-int coord |
| 0x774 | 4 | `ParticleSystemType*` | `RefinerySmokeParticleSystem` | nullptr | |
| 0x778 | 0x1C | vec\<ParticleSystem\*\> | `DamageParticleSystems` | empty | Tokenized |
| 0x794 | 0x1C | vec\<ParticleSystem\*\> | `DestroyParticleSystems` | empty | Tokenized |
| 0x7B0..0x7B8 | 3×4 | int[3] | `DamageSmokeOffset` | 0,0,0 | |
| 0x7BC | 1 | bool | `DamSmkOffScrnRel` | false | |
| 0x7BC | 4 | int (enum) | `SpeedType` | -1 | **Offset alias — same 4 bytes** (overlapped interp; rare engine quirk) |
| 0x7C0..0x7C8 | 3×4 | int[3] | `DestroySmokeOffset` | 0,0,0 | |
| 0x7CC..0x7D4 | 3×4 | int[3] | `RefinerySmokeOffsetOne` | 0,0,0 | |
| 0x7D8..0x7E0 | 3×4 | int[3] | `RefinerySmokeOffsetTwo` | 0,0,0 | |
| 0x7E4..0x7EC | 3×4 | int[3] | `RefinerySmokeOffsetThree` | 0,0,0 | |
| 0x7F0..0x7F8 | 3×4 | int[3] | `RefinerySmokeOffsetFour` | 0,0,0 | |
| 0x7FC | 4 | int | `ShadowIndex` | 0 | |
| 0x800 | 4 | int | `Storage` | 0 | |
| 0x804 | 1 | bool | `TurretNotExportedOnGround` | false | |
| 0x805 | 1 | bool | `Gunner` | false | |
| 0x806 | 1 | bool | `HasTurretTooltips` | false | |
| 0x808 | 4 | int | `TurretCount` | 0 | |
| 0x80C | 4 | int | `WeaponCount` | 0 | |
| 0x810 | 1 | bool | `IsChargeTurret` | false | |

**Weapon slot block** — 18× 0x1C-byte `WeaponStruct` entries at `[0x85C .. 0xA8F]`
(5 AlternateFLH entries) and primary weapon block at `[0x898 .. 0xA8F]`:

| Offset | Field | INI Key | Notes |
|--------|-------|---------|-------|
| 0x85C..0x888 | `AlternateFLH[0..4]` 3-int triples | `AlternateFLH0..4` | 5 triples, 12 bytes each |
| 0x898..0x8B3 | `Primary` + FLH + BarrelLength + BarrelThickness | `Primary`, `PrimaryFireFLH`, `PBarrelLength`, `PBarrelThickness` | |
| 0x898+28n (loops when Gattling) | `Weapon%d`, `Weapon%dFLH`, `Weapon%dBarrelLength`, etc. | `Weapon1..N` | Stride 28 bytes |
| 0x8B4..0x8CF | `Secondary` + FLH + barrel | `Secondary`, `SecondaryFireFLH`, `SBarrelLength`, `SBarrelThickness` | |
| 0xA90 | `ClearAllWeapons` | `ClearAllWeapons` | If true, zeros weapons after read |
| 0xA94..0xAAF | `ElitePrimary` + FLH + barrel | `ElitePrimary`, `ElitePrimaryFireFLH`, `ElitePBarrelLength`, `ElitePBarrelThickness` | |
| 0xA94+28n (Gattling) | `EliteWeapon%d...` | `EliteWeapon1..N` | Stride 28 |
| 0xAB0..0xAC7 | `EliteSecondary` + FLH + barrel | `EliteSecondary`, `EliteSecondaryFireFLH`, `EliteSBarrelLength`, `EliteSBarrelThickness` | |
| 0xAB8..0xAC9 | `EliteAbilities` | `EliteAbilities` | **byte[18]** — same layout as VeteranAbilities |

**Misc behavioral flags** (`[0xC8C .. 0xDCF]`):

| Offset | Size | Field | INI Key | Default |
|--------|------|-------|---------|---------|
| 0xC8C | 1 | bool | `TypeImmune` | false |
| 0xC8D | 1 | bool | `MoveToShroud` | false (ctor=1) |
| 0xC8E | 1 | bool | `Trainable` | false (ctor=1) |
| 0xC8F | 1 | bool | (Infantry patches this: IsHero/similar flag) | 0 |
| 0xC90 | 1 | bool | `TargetLaser` | false |
| 0xC91 | 1 | bool | `ImmuneToVeins` | false |
| 0xC92 | 1 | bool | `TiberiumHeal` | false |
| 0xC93 | 1 | bool | `CloakStop` | false |
| 0xC94 | 1 | bool | `IsTrain` | false |
| 0xC95 | 1 | bool | `IsDropship` | false |
| 0xC96 | 1 | bool | `ToProtect` | false |
| 0xC97 | 1 | bool | `Disableable` | **true** |
| 0xC99 | 1 | bool | `DoubleOwned` | false |
| 0xC9A | 1 | bool | `Invisible` | false |
| 0xC9B | 1 | bool | `RadarVisible` | false |
| 0xC9C | 1 | bool | (PrimaryWeapon burst flag cache) | 0 |
| 0xC9D | 1 | bool | `Sensors` | false |
| 0xC9E | 1 | bool | `Nominal` | false |
| 0xC9F | 1 | bool | `DontScore` | false |
| 0xCA0 | 1 | bool | `DamageSelf` | false |
| 0xCA1 | 1 | bool | `Turret` | false |
| 0xCA2 | 1 | bool | `TurretRecoil` | false |
| 0xCA4..0xCB4 | 5×4 | int[5] | `TurretTravel/Compress/Hold/Recover/Post` | 2,1,1,1,0 | Via recoil helpers |
| 0xCB8..0xCC8 | 5×4 | int[5] | `BarrelTravel/Compress/Hold/Recover/Post` | 2,1,1,1,0 | Same pattern; Turret block memcpy'd first, then overrides |
| 0xCCC | 1 | bool | `Repairable` | **true** |
| 0xCCD | 1 | bool | `Crewed` | false |
| 0xCCE | 1 | bool | `Naval` | false |
| 0xCCF | 1 | bool | `Remapable` | false |
| 0xCD0 | 1 | bool | `Cloakable` | false |
| 0xCD1 | 1 | bool | `GapGenerator` | false |
| 0xCD2 | 1 | byte | `GapRadiusInCells` | 0 | Stored as byte |
| 0xCD3 | 1 | byte | `SuperGapRadiusInCells` | 0 | Stored as byte |
| 0xCD4 | 1 | bool | `Teleporter` | false |
| 0xCD5 | 1 | bool | `IsGattling` | false |
| 0xCD8 | 4 | int | `WeaponStages` | 0 |
| 0xCE4+4n | int | `Stage%d` (n=1..WeaponStages-1) | (loop) | |
| 0xCF4+4n | int | `EliteStage%d` | (loop) | |
| 0xD0C | 4 | int | `RateUp` | 0 |
| 0xD10 | 4 | int | `RateDown` | 0 |
| 0xD14 | 1 | bool | `SelfHealing` | false |
| 0xD15 | 1 | bool | `Explodes` | false |
| 0xD18 | 4 | `WeaponType*` | `DeathWeapon` | nullptr |
| 0xD1C | 4 | float | `DeathWeaponDamageModifier` | 1.0 |
| 0xD20 | 1 | bool | `NoAutoFire` | false |
| 0xD21 | 1 | bool | `TurretSpins` | false |
| 0xD22 | 1 | bool | `TiltCrashJumpjet` | false |
| 0xD23 | 1 | bool | `Normalized` | false |
| 0xD24 | 1 | bool | `ManualReload` | false |
| 0xD25 | 1 | bool | `VisibleLoad` | false |
| 0xD26 | 1 | bool | `LightningRod` | false |
| 0xD27 | 1 | bool | `HunterSeeker` | false |
| 0xD28 | 1 | bool | `Crusher` | false |
| 0xD29 | 1 | bool | `OmniCrusher` | false |
| 0xD2A | 1 | bool | `OmniCrushResistant` | false |
| 0xD2B | 1 | bool | `TiltsWhenCrushes` | **true** |
| 0xD2C | 1 | bool | (cached `IsWaterBound` from MovementZone==6) | 0 |
| 0xD2D | 1 | bool | `AutoCrush` | false |
| 0xD2E | 1 | bool | `Bunkerable` | false |
| 0xD2F | 1 | bool | `CanDisguise` | false |
| 0xD30 | 1 | bool | `PermaDisguise` | false |
| 0xD31 | 1 | bool | `DetectDisguise` | false |
| 0xD32 | 1 | bool | `DisguiseWhenStill` | false |
| 0xD33 | 1 | bool | `CanApproachTarget` | **true** |
| 0xD34 | 1 | bool | `CanRecalcApproachTarget` | **true** |
| 0xD35 | 1 | bool | `ImmuneToPsionics` | false |
| 0xD36 | 1 | bool | `ImmuneToPsionicWeapons` | false |
| 0xD37 | 1 | bool | `ImmuneToRadiation` | false |
| 0xD38 | 1 | bool | `Parasiteable` | false |
| 0xD39 | 1 | bool | `DefaultToGuardArea` | false |
| 0xD3A | 1 | bool | `Warpable` | **true** |
| 0xD3B | 1 | bool | `ImmuneToPoison` | false |
| 0xD3C | 1 | bool | `ReselectIfLimboed` | false |
| 0xD3D | 1 | bool | `RejoinTeamIfLimboed` | false |
| 0xD3E | 1 | bool | `Slaved` | false |
| 0xD40 | 4 | `InfantryType*` | `Enslaves` | nullptr |
| 0xD44 | 4 | int | `SlavesNumber` | 0 |
| 0xD48 | 4 | int | `SlaveRegenRate` | 0 |
| 0xD4C | 4 | int | `SlaveReloadRate` | 0 |
| 0xD50 | 4 | int | `OpenTransportWeapon` | -1 |
| 0xD54 | 1 | bool | `Spawned` | false |
| 0xD58 | 4 | `AircraftType*` | `Spawns` | nullptr |
| 0xD5C | 4 | int | `SpawnsNumber` | 0 |
| 0xD60 | 4 | int | `SpawnRegenRate` | 0 |
| 0xD64 | 4 | int | `SpawnReloadRate` | 0 |
| 0xD68 | 1 | bool | `MissileSpawn` | false |
| 0xD69 | 1 | bool | `Underwater` | false |
| 0xD6A | 1 | bool | `BalloonHover` | false |
| 0xD6C | 4 | int | `SuppressionThreshold` | 0 |
| 0xD70 | 4 | int | `JumpjetTurnRate` | 4 |
| 0xD74 | 4 | int | `JumpjetSpeed` | 14 |
| 0xD78 | 4 | float | `JumpjetClimb` | 5.0 |
| 0xD7C | 4 | float | `JumpjetCrash` | 5.0 |
| 0xD80 | 4 | int | `JumpjetHeight` | 500 |
| 0xD84 | 4 | float | `JumpjetAccel` | 2.0 |
| 0xD88 | 4 | float | `JumpjetWobbles` | 0.15 |
| 0xD8C | 1 | bool | `JumpjetNoWobbles` | false |
| 0xD90 | 4 | int | `JumpjetDeviation` | 40 |
| 0xD94 | 1 | bool | `JumpJet` | false |
| 0xD95 | 1 | bool | `Crashable` | false |
| 0xD96 | 1 | bool | `ConsideredAircraft` | false |
| 0xD97 | 1 | bool | `Organic` | false |
| 0xD98 | 1 | bool | `NoShadow` | false |
| 0xD99 | 1 | bool | `CanPassiveAquire` | **true** |
| 0xD9A | 1 | bool | `CanRetaliate` | **true** |
| 0xD9B | 1 | bool | `RequiresStolenThirdTech` | false |
| 0xD9C | 1 | bool | `RequiresStolenSovietTech` | false |
| 0xD9D | 1 | bool | `RequiresStolenAlliedTech` | false |
| 0xDA0 | 4 | uint | `RequiredHouses` | 0xFFFFFFFF | bitmask (§6.7) |
| 0xDA4 | 4 | uint | `ForbiddenHouses` | 0xFFFFFFFF | bitmask |
| 0xDA8 | 4 | uint | `SecretHouses` | 0xFFFFFFFF | bitmask |
| 0xDAC | 1 | bool | `UseBuffer` | false |
| 0xDB0..0xDB8 | 3×4 | int[3] | `SecondSpawnOffset` | 0,0,0 |
| 0xDBC | 1 | bool | `IsSelectableCombatant` | false |
| 0xDBD | 1 | bool | `Accelerates` | **true** |
| 0xDBE | 1 | bool | `DisableVoxelCache` | false |
| 0xDBF | 1 | bool | `DisableShadowCache` | false |
| 0xDC0 | 4 | int | `ZFudgeCliff` | 10 |
| 0xDC4 | 4 | int | `ZFudgeColumn` | 5 |
| 0xDC8 | 4 | int | `ZFudgeTunnel` | 10 |
| 0xDCC | 4 | int | `ZFudgeBridge` | 0 |
| 0xDD0 | 32 | char[32] | `Palette` | `""` | Re-read from INI each Load; palette loaded into 0xDF0 |
| 0xDF0 | 4 | `PaletteClass*` | Palette object | nullptr | **Runtime-derived** |
| 0xDF4 | 4 | (reserved / padding) | — | — | Not touched by ctor or ReadINI; present for alignment |

**Last base field is `[0x37D]` at byte 0xDF4 (reserved). Subclass fields begin at `[0x37E]` (byte 0xDF8).**

---

## 6. Enum Tables

### 6.1 Armor (ObjectType) — parser `FUN_004753F0`, table at `0x007E5210`

| Index | String |
|------:|--------|
| 0 | `none` |
| 1 | `flak` |
| 2 | `plate` |
| 3 | `light` |
| 4 | `medium` |
| 5 | `heavy` |
| 6 | `wood` |
| 7 | `steel` |
| 8 | `concrete` |
| 9 | `special_1` |
| 10 | `special_2` |

Unknown/absent → index 0 (`none`). Default input to ReadString is the currently
stored armor name (via `(&PTR_DAT_007e5210)[current]`), so absent key preserves
prior value.

**TS-era tokens absent from string table:** `aluminum`, `wood2`, `concrete2`. Confirmed.

### 6.2 Prerequisite keyword table — parser `Prerequisite_INI_Parser` @ ~`0x004770E0`

| Token | Written value | 2's complement |
|-------|--------------:|---------------:|
| `POWER` | `0xFFFFFFFF` | -1 |
| `FACTORY` | `0xFFFFFFFE` | -2 |
| `BARRACKS` | `0xFFFFFFFD` | -3 |
| `RADAR` | `0xFFFFFFFC` | -4 |
| `PROC` | `0xFFFFFFFB` | -5 |
| `TECH` | `0xFFFFFFFA` | -6 |
| else | BuildingType index | ≥ 0 |

Case-sensitive string comparison. Non-keyword tokens resolved via
`BuildingTypeClass::FindIndexByID` (linear scan of `g_BuildingTypeClass_Array`
against field `+0x24`). Unknown names → silently skipped (not added to the
vector). Tokenization uses `strtok(",")`. Stored into the `Prerequisite` or
`PrerequisiteOverride` DynamicVectorClass at `+0x638` / `+0x654`.

### 6.3 PipScale — parser `FUN_00474940`, table at `0x0081B9B0`

| Index | String |
|------:|--------|
| 1 | `Ammo` |
| 2 | `Tiberium` |
| 3 | `Passengers` |
| 4 | `Power` |
| 5 | `MindControl` |

Stored at TechnoType `+0x3D4`. Default / unrecognized → 0 (no pips).

### 6.4 VeteranAbilities / EliteAbilities — parser `FUN_00477640`, table at `0x008463B8`

**Storage: `byte[18]` array** (one byte per ability, value 0 or 1). NOT a
bitmask. Veteran at `[0x29C .. 0x2AD]`, Elite at `[0xAB8 .. 0xAC9]`.

| Index | Ability name |
|------:|--------------|
| 0 | `FASTER` |
| 1 | `STRONGER` |
| 2 | `FIREPOWER` |
| 3 | `SCATTER` |
| 4 | `ROF` |
| 5 | `SIGHT` |
| 6 | `CLOAK` |
| 7 | `TIBERIUM_PROOF` |
| 8 | `VEIN_PROOF` |
| 9 | `SELF_HEAL` |
| 10 | `EXPLODES` |
| 11 | `RADAR_INVISIBLE` |
| 12 | `SENSORS` |
| 13 | `FEARLESS` |
| 14 | `C4` |
| 15 | `TIBERIUM_HEAL` |
| 16 | `GUARD_AREA` |
| 17 | `CRUSHER` |

Tokenization: `strtok(",")`. Unknown tokens silently skipped. Missing key →
copies from previous/default value struct.

**TS-era holdovers**: `TIBERIUM_PROOF`, `VEIN_PROOF`, `TIBERIUM_HEAL` — the
ability names survived; their effect paths gate against `TiberiumHeal` flag at
`+0xC92` and vein logic (dormant in YR). Parsing the ability is live; the
effect is conditional on dormant TS systems.

### 6.5 Category — parser `FUN_004749E0`, table at `0x0081B7C8`

| Index | Short name | Long name |
|------:|------------|-----------|
| 0 | `Soldier` | `Soldier` |
| 1 | `Civilian` | `Civilian` |
| 2 | `VIP` | `VIP/Agent` |
| 3 | `Recon` | `Recon Vehicle` |
| 4 | `AFV` | `Armored Fighting Vehicle` |
| 5 | `IFV` | `Infantry Fighting Vehicle` |
| 6 | `LRFS` | `Indirect Fire Support` |
| 7 | `Support` | `Misc. Support Vehicle` |
| 8 | `Transport` | `Transport Vehicle` |
| 9 | `AirPower` | `Air Combat Support` |
| 10 | `AirLift` | `Air Transport` |

Stored at `+0x3BC`. Unknown → -1. ReadString default = long-name of current
value.

### 6.6 VHPScan — parser `FUN_00477590`

| Token | Value |
|-------|------:|
| `None` | 0 |
| `Normal` | 1 |
| `Strong` | 2 |
| else / missing | previous/default |

Stored at `+0x394`.

### 6.7 Owner / RequiredHouses / SecretHouses / ForbiddenHouses bitmask — `FUN_004750D0`

**Mapping is DATA-DRIVEN, not hardcoded.** For each comma-separated token:

1. `strtok(",")` splits the list.
2. Each token → `FUN_005117D0` (`HouseTypeClass::FindIndexByName`): linear scan
   of `g_HouseTypeClass_Array[0 .. DAT_00A83CA8-1]` comparing token to ID
   (`+0x64`) and short name (`+0x24`). On match, returns the HouseType's
   **`+0xB8` field** — which is the house's *bit index*, assigned during rules
   load.
3. Special: `<random>` returns `-2` → bit 30 (`1 << 30`).
4. Unknown token → returns `-1` → silently contributes 0 (no flag raised).
5. Result: `mask |= (1 << (index & 0x1F))` accumulated over all tokens.

Stored at `+0x6CC` (Owner), `+0xDA0` (RequiredHouses), `+0xDA4`
(ForbiddenHouses), `+0xDA8` (SecretHouses).

**Country → bit mapping is therefore INI-order-dependent.** In stock YR
`[Countries]` list:
`Americans=0, Alliance=1, French=2, Germans=3, British=4, Africans=5, Arabs=6,
Confederation=7, Russians=8, YuriCountry=9, ...`.

---

## 7. Virtual Methods

### 7.1 `TechnoTypeClass::Cost_Of` @ `0x00711EC0` (vtable slot 28)

```pseudo
int Cost_Of(this) {
    if (this->[0xC99] & 1)  // (flag byte — likely DoubleOwned or similar)
        return INT_MAX;     // "unbuildable" sentinel
    return this->[0x6CC];    // Owner bitmask? — see note
}
```

**Note:** Reads `+0x6CC` (Owner bitmask). Either (a) this is the wrong method
name and this is actually `Get_Ownable`, or (b) there's a second read that the
scoping decompilation missed. Downstream production code does read Cost from
`+0x610` inline; the role of `0x00711EC0` in practice deserves one more
trace-through. Confidence: MEDIUM on "this is Cost_Of".

### 7.2 `TechnoTypeClass::GetBuildTime` @ `0x00711EE0`

```asm
FILD    [this + 0x610]              ; load Cost (int)
FMUL    [g_RulesClass_Instance + 0x1748]  ; × Rules.BuildSpeed (double)
FMUL    [0x007F4E80]                ; × 0.9 (hard constant)
JMP     Math::ftol                  ; round to int
```

**Formula:** `BuildTime = ftol(Cost × Rules.BuildSpeed × 0.9)`. Returns game
ticks (frames). No country multipliers applied here — those are applied by
callers in the HouseClass production path. No minimum clamp. Hard constant is
IEEE 754 `0x3FECCCCCCCCCCCCD`.

### 7.3 `TechnoTypeClass::GetFlightLevel` @ `0x00717800` (corrected 2026-05-28: was `GetSpeed`; Ghidra label is `TechnoTypeClass__GetFlightLevel` via `decompile_function 0x00717800`; field `+0x618` is `FlightLevel` not base speed — ROOT_CAUSE: RTTI_LABEL_DRIFT)

```c
int GetFlightLevel(this) {
    int s = this->[0x618];  // FlightLevel field
    return (s == -1) ? g_RulesClass_Instance->[0x7b4] : s;
}
```

Returns `FlightLevel` (the altitude at which this type flies). If -1, falls
back to the global default flight level from `RulesClass+0x7B4`. Not the unit's
movement speed — callers seeking speed read `+0x618` only for the flight-altitude
path.

### 7.4 `TechnoTypeClass::Load` @ `0x007162F0` (vtable slot 5)

Reads from IStream. Structure:

1. Pre-phase: calls vtable slot `+0xC` (release/reset) on 18 embedded
   sub-objects at fixed offsets `[0x314, 0x330, 0x3E8, 0x414, 0x430, 0x44C,
   0x468, 0x484, 0x4A0, 0x4BC, 0x4D8, 0x4F4, 0x510, 0x638, 0x654, 0x72C,
   0x748, 0x5C4]`.
2. Super-call: `ObjectTypeClass::Load` @ `0x005F9720` (bails on non-zero
   HRESULT).
3. Stream-read: 18 variable-length lists. Each list = `count (int32) + count ×
   int32`. Counts land at `[0x324, 0x340, 0x3F8, 0x424, 0x440, 0x45C, 0x478,
   0x494, 0x4B0, 0x4CC, 0x4E8, 0x504, 0x520, 0x5D4, 0x73C, 0x758, 0x788,
   0x7A4]`.
4. Pointer fixups: 12 `FUN_006cf240` calls for scalar pointer fields at
   `[0x404, 0x408, 0x40C, 0x764, 0x774, 0xD58, 0xD40, 0xD18, 0x624, 0x628,
   0x6B8, 0x6BC]`.
5. **Re-read from INI+MIX (NOT streamed):** `Cameo=` → SHP at `+0x6F0`;
   `AltCameo=` → SHP at `+0x710`; `Palette=` → `+0xDD0` + palette at `+0xDF0`;
   parent Image SHP at `+0xA4`.

### 7.5 `TechnoTypeClass::Save` @ `0x00716DC0` (vtable slot 6)

Mirror of Load for the 18 variable-length lists. Does NOT write Cameo/AltCameo/
Palette/Image SHP — those are non-stateful runtime pointers.

### 7.6 `TechnoTypeClass::GetSizeMax` @ `0x007170A0` (vtable slot 7)

Calls `ObjectTypeClass::GetSizeMax` @ `0x005F9970`. Increments running size by
`4 + count × 4` for each of the 18 variable-length list count fields. Matches
Load/Save enumeration.

### 7.7 `TechnoTypeClass::Compute_CRC` @ `0x007171A0` (vtable slot 13)

Calls `AbstractTypeClass::Compute_CRC` @ `0x00410BE0`. Folds ~100 scalar
fields (coords, floats, ints, bools) into the CRC. For the 18 variable-length
lists, folds only the COUNT field, not the contents. Cameo/AltCameo/Palette/
Image SHP pointers are NOT folded (confirming their non-stateful nature).

### 7.8 `~TechnoTypeClass` (scalar-deleting dtor) @ `0x007179A0` (vtable slot 8)

Standard MSVC two-line thunk:

```c
ret ~TechnoTypeClass::non_virtual_dtor(this);
if (flag & 1) operator_delete(this);
```

No TS-era conditional teardown branches. Inline sub-object dtors live inside
the non-virtual dtor (not decompiled in this pass — it destroys the 20+ inline
VectorClass subobjects via their slot-8 dtor virtual).

### 7.9 Methods NOT found as standalone (inlined)

| Method | Status | Evidence |
|--------|--------|----------|
| `Who_Can_Build_Me` | Inlined in `HouseClass::CanBuild @ 0x004F7870` | Reads `TechnoType[0x368]` (byte offset 0xDA0 — RequiredHouses) directly |
| `Factory_Of_Kind` | Inlined across HouseClass methods; no single function | Related helper: `HouseClass::GetPrimaryFactoryBuilding @ 0x004FBD80` |
| `Prereq_Needed` | Inlined in `HouseClass::CanBuild` | Switch on -1..-6 keywords resolves against `Rules.PrerequisitePower/Factory/Barracks/Radar/Tech/ProcAlternate`; default → `BuildingType` index scan |
| `Get_Ownable` | Inlined | Every caller reads `TechnoType+0xDA0` and ANDs with HouseClass side/country bit directly |

The Westwood C++ style inlined these accessors for speed. Any Rust port should
replicate the inline reads, not synthesize standalone methods.

---

## 8. Super-Call Invariants (Subclass ReadINI)

All four subclass ReadINI methods follow the same contract:

```c
bool <Sub>TypeClass::Read_INI(this, ini) {
    if (!TechnoTypeClass::Read_INI(this, ini)) return false;
    // ... subclass-specific reads ...
    return true;
}
```

### Confirmed super-call sites

| Subclass | ReadINI Address | Super-call site | Bails on 0? | First subclass field |
|----------|-----------------|-----------------|-------------|----------------------|
| UnitTypeClass | `0x00747620` | ≈ entry | ✓ | `[0xDF8]` (actually the shared kind-index/WaterBound area; subsequent fields ≥0xDFC) |
| InfantryTypeClass | `0x005240A0` | ≈ entry | ✓ | `[0xDFC]` |
| BuildingTypeClass | **`0x0045FE50`** (NOT `0x006F32D0`) | ≈ entry | ✓ | `[0xE08]` |
| AircraftTypeClass | `0x0041CC20` | ≈ entry | ✓ | `[0xDFC]` |

**Correction:** The plan doc's `0x006F32D0` for BuildingTypeClass::Read_INI is
wrong. That address is a ~90-byte unrelated predicate. The real ReadINI is at
`0x0045FE50`, currently mislabeled in Ghidra as `BuildingTypeClass_ReadINI_Water`.
This is a labeling bug in the current Ghidra DB — do not rename without
verification per CLAUDE.md's ~90% rule, but flag for future attention.

### Subclass instance sizes

| Subclass | Highest offset | Heap extras | Total est. |
|----------|---------------:|------------|-----------:|
| UnitTypeClass | 0xE5E | — | ~0xE80 |
| InfantryTypeClass | 0xECB | `operator_new(0x5E8)` at `[0x38F]` | ~0xED0 main + heap |
| BuildingTypeClass | 0x1791 | `operator_new(0xC)` at `[0x5E2]` | ~0x1794 main + heap |
| AircraftTypeClass | 0xE0E | — | ~0xE10 |

### Subclass patches into base range

A few subclass ReadINI calls write into the 0x294..0xDF7 base range — these
are *deliberate* subclass-specific default overrides, not bugs:

- UnitTypeClass writes `+0x67C` (kind-index field; UnitType stores SpeedType default there)
- UnitTypeClass writes `+0x398` (harvester PipScale override)
- BuildingTypeClass writes `+0x67C` (WaterBound flag — buildings re-use the same kind-index DWORD)
- InfantryTypeClass writes `+0xC8F` (unnamed bool in the flag cluster, possibly IsHero-adjacent)

### VoiceComment confirmed InfantryType-only

`VoiceComment` reads into InfantryType offsets `[0xE98, 0xE9C, 0xEA0]` — all
above the base boundary. Not a base TechnoType field. Safe to keep
InfantryType-specific in any Rust model.

---

## 9. Constructor Notes

### 9.1 TechnoTypeClass constructor @ `0x00710AF0`

Signature: `TechnoTypeClass__Constructor(this, id_string, kind_index)`.

- Super-calls `ObjectTypeClass::ctor(this, id_string)` first.
- `id_string` is passed up through AbstractTypeClass → stored in `Name` at
  `[0x64]`; if nullptr, a default `%08X` hex-stringified-this-pointer is
  synthesized.
- `kind_index` (third arg) is stored at `[0x19F]` = byte `0x67C`.
- Overwrites 4 vtable slots at 0x00/0x04/0x08/0x0C with TechnoType vtable
  addresses.
- Initializes ~110 scalar fields inline.
- Constructs ~20 embedded DynamicVectorClass / VectorClass subobjects via
  helpers `FUN_0067C310`, `FUN_00477BE0`, `FUN_005105A0`, `FUN_00525680`,
  `FUN_0045AD80`, `FUN_00717AF0`. All helpers install a vtable + zero the
  count/capacity/owns-memory fields. No heap allocation happens in the ctor
  itself (all capacity args are 0).
- `strncpy`s two 24-byte inline string buffers at `[0x6D6]` and `[0x6F5]`
  from `DAT_00889F64` (empty placeholder).
- Zero-fills three parallel inline tables: 18 records × 7 DWORDs at
  `[0x888]..[0xA84]`, another 18 × 7 at `[0xA8C]..[0xC84]`, and 5 records × 3
  DWORDs at `[0x85C]..[0x894]`. The two 18×7 tables are the per-weapon-slot
  WeaponStruct arrays (primary + elite).
- Writes 18 `-1` indices at `[0x205]..[0x216]` and 35 `-1` indices at
  `[0x14B]..[0x16C]`. The 18-slot "-1" bank is consistent with per-house state.
- Appends `this` to two global DynamicVectorClass registries (all-TechnoType
  index + RTTI registry).

### 9.2 Non-obvious ctor defaults (from the layout table's "Default" columns)

- `Disableable` defaults **true** at `[0xC97]`.
- `Bombable` defaults **true** at `[0x22E]`.
- `Selectable` / `LegalTarget` default **true**.
- `Repairable` defaults **true**.
- `TiltsWhenCrushes` defaults **true**.
- `CanApproachTarget` / `CanRecalcApproachTarget` / `CanPassiveAquire` /
  `CanRetaliate` default **true**.
- `Warpable` defaults **true**.
- `Accelerates` defaults **true**.
- `AllowedToStartInMultiplayer` defaults **true**.
- `CanBeHidden` defaults **true**.
- `MobileFire` defaults **true**.
- `MoveToShroud` / `Trainable` default **true**.
- `Ammo` / `InitialAmmo` / `EmptyReload` / `UndeployDelay` default `-1`
  (sentinel).
- `BuildLimit` defaults `INT_MAX`.
- `TechLevel` defaults `255`.
- `SlowdownDistance` defaults `500`.
- `JumpjetHeight` defaults `500`.
- `Weight` defaults `2.0`.
- `PhysicalSize` defaults `1.0`.
- `AccelerationFactor` defaults ~`0.030`.
- `DeaccelerationFactor` defaults ~`0.002`.

Many of these "default true" flags are counter-intuitive and worth noting for
the Rust audit — writing `IsSelectable=no` in rulesmd.ini is a toggle-off, not
a toggle-on.

---

## 10. TS-Legacy Register — Active in YR verdict

Per-field verdict for every TS-flavored name discovered in the ReadINI chain:

| Field | Offset | Read | YR Active? | Evidence |
|-------|-------:|------|------------|----------|
| `TiberiumImmunity` | — | **NOT READ** | No | String ABSENT from binary string table — confirmed via `list_strings` scan |
| `CostLower` | — | **NOT READ** | No | String ABSENT from binary string table |
| Old armor tokens (`aluminum`, `wood2`, `concrete2`) | — | not parsed | No | Strings ABSENT — YR armor enum has 11 entries, all current names |
| `Immune` | 0x233 | read (ObjectType) | **Yes (LIVE)** | Checked on hot damage path; iron-curtain / chronosphere dependencies |
| `TiberiumExplosive` | — | NOT on ObjectType/TechnoType | Conditional | Lives on Warhead/Rules — out of scope here |
| `TiberiumProof` | — | NOT on ObjectType/TechnoType | Conditional | InfantryType-only property (subclass ReadINI) |
| `TiberiumHeal` | 0xC92 | read (TechnoType) | **Dormant** | Parsed & stored; tiberium not present in YR, no live consumer |
| `ImmuneToVeins` | 0xC91 | read | **Dormant** | Veins = TS-only terrain; parsed but unused |
| `IsTrain` | 0xC94 | read | **Dormant** | No trains in stock YR |
| `IgnoresFirestorm` | 0x239 | read (ObjectType) | **Dormant** | Firestorm wall mechanic off by default in YR (SpecialFlags gate) |
| `AlternateArcticArt` | 0x211 | read (ObjectType) | Conditional | Active only on arctic theaters; YR missions generally don't use |
| `StupidHunt` | 0x6D4 | read | Conditional | AI flag; still referenced but seldom set in stock content |
| `HunterSeeker` | 0xD27 | read | Conditional | Superweapon exists in YR but is special-cased |
| `LightningRod` | 0xD26 | read | **Live** | Used by Weather Control SW |
| `Sensors` (byte @ 0xC9D) + `SensorsSight` (int @ 0x5F0) | 0xC9D/0x5F0 | read | **Live** | Used by Robot Tanks / Psychic Sensor |
| `DeploysInto` / `UndeploysInto` | 0x404/0x408 | read | **Live** | MCV → Construction Yard chain |
| VeteranAbility tokens `TIBERIUM_PROOF` / `VEIN_PROOF` / `TIBERIUM_HEAL` | §6.4 | parsed | Conditional | Parser accepts them; effect paths gate against dormant TS systems |

**Summary:** Every truly TS-era key name is either absent from the binary
(`TiberiumImmunity`, `CostLower`, old armor tokens) OR is read into a field
that's been repurposed / gated dormant in YR. The one field that survived
wholesale (`Immune`) is genuinely live in YR — don't treat the name as proof
of TS-legacy.

---

## 11. Current Rust Implementation Surface

Today, [src/rules/object_type.rs](../ra2-rust-game/src/rules/object_type.rs)
collapses UnitType / InfantryType / BuildingType / AircraftType into a single
`ObjectType` struct discriminated by an `ObjectCategory` enum. Driver at
[src/rules/ruleset.rs](../ra2-rust-game/src/rules/ruleset.rs).

### Parse coverage audit — base TechnoType keys

Spot-check against the Phase 1c Rust implementation scan:

| Base key | In `ObjectType`? | Notes |
|----------|------------------|-------|
| Cost | ✓ | |
| Speed | ✓ | |
| Armor | ✓ | |
| Strength | ✓ | |
| Sight | ✓ | |
| Owner | ✓ | Likely stored as Vec<HouseType> not bitmask — OK for semantics |
| Prerequisite | ✓ | |
| Cloakable | ✓ | |
| Primary / Secondary | ✓ | |
| VoiceSelect / VoiceMove / VoiceAttack / DieSound / MoveSound | ✓ | |
| Foundation / Power / Ammo | ✓ | |
| Locomotor | ✓ | |
| SpeedType / MovementZone | ✓ | |
| Category | Gap suspected | No Category enum field seen; may be missing |
| VHPScan | Gap suspected | |
| ROT / RotCount | Gap suspected | |
| TechLevel | Check | |
| PipScale | ✓ (enum defined) | |
| All 30+ Voice*/*Sound VOC-index fields | Partial | Full audit pending |
| VeteranAbilities / EliteAbilities | Gap suspected | 18-byte flag array likely not modeled |
| Weapon/EliteWeapon loop (`Weapon%d`, `Stage%d`, `AlternateFLH%d`) | Gap suspected | |
| ZFudge fields | Gap suspected | |
| Jumpjet\* bundle | Gap suspected | |
| Slave\* / Spawn\* families | Gap suspected | |
| Threat coefficients (5× double) | Gap suspected | |

A complete diff-audit of every base-key → Rust-field mapping is out of scope
for this report; use it as the reference for the audit.

---

## 12. Open Questions

1. **`Cost_Of` vs `Get_Ownable` confusion at `0x00711EC0`** — the function
   reads `+0xC99` and `+0x6CC`. The latter is the Owner bitmask, not Cost
   (`+0x610`). The function may actually be `Get_Ownable_By_House` or a
   combined check. MEDIUM-confidence label; one more trace needed.
2. **Unnamed Ctor magic constants** — several ctor init values (e.g.,
   `0x3F800000` at `[0xD1C]` = DeathWeaponDamageModifier 1.0; sight=500 at
   `[0xBE]`) match known fields, but a small residue (e.g., `0xE = 14` at
   `[0x35D]` for JumpjetSpeed default) warrants cross-check against rules.ini.
3. **The `[0x888]..[0xA84]` and `[0xA8C]..[0xC84]` inline 18×7-DWORD tables**
   — scoping identified them as per-weapon WeaponStructs (18 slots × 7 DWORDs)
   and parallel elite bank. The per-slot structure is inferred from loop
   strides and the 5-accessor recoil helper pattern (`FUN_00717a30..00717ad0`);
   a dedicated Phase 4 pass should decode each slot layout authoritatively.
4. **`FUN_00717ae0`** — "primary-weapon voxel-availability check" called after
   ReadINI. Writes to `[0x360]` / `[0x368]` (voxel hotspot floats). Minor
   gameplay effect unknown.
5. **`CDFileClass` branch at `0x71608a`** — conditional CD-asset stub call;
   purpose in modern YR (which doesn't need CD asset fallback) unclear. Likely
   dormant.
6. **Vtable slot 28 identity** — `0x00711EC0` is either `Cost_Of` or a distinct
   accessor. Resolve via caller trace.
7. **VectorClass vtable discrepancies** — Phase 2b found that `FUN_00525680`
   installs `0x007EB6F4` (not `0x007EB6D4` as an earlier note suggested) and
   `FUN_0045AD80` installs `0x007E4424`. These mismatches don't affect
   behavior but should be reconciled with any prior VectorClass research.
8. **Subclass base-range patches** (UnitType +0x67C/+0x398, BuildingType
   +0x67C, InfantryType +0xC8F) — verify these are intentional default-overrides
   and not bugs. Likely fine (each subclass has its own production/deployment
   semantics), but worth confirming by inspecting the values written.

---

## Sources

- **Ghidra MCP (live gamemd.exe, image base 0x00400000):**
  - `TechnoTypeClass::Read_INI @ 0x00712170` — fully decompiled
  - `TechnoTypeClass::Constructor @ 0x00710AF0` — fully decompiled
  - `ObjectTypeClass::Read_INI @ 0x005F92D0` — fully decompiled
  - `ObjectTypeClass::Constructor @ 0x005F7090` — decompiled
  - `AbstractTypeClass::Read_INI @ 0x00410A60` (was `FUN_00410A60`) — decompiled
  - `AbstractTypeClass::Constructor @ 0x00410800` — decompiled
  - `TechnoTypeClass::Load @ 0x007162F0`, `Save @ 0x00716DC0`, `GetSizeMax @ 0x007170A0`, `Compute_CRC @ 0x007171A0`, `~TechnoTypeClass @ 0x007179A0` — decompiled
  - `TechnoTypeClass::GetBuildTime @ 0x00711EE0`, `GetSpeed @ 0x00717800`, `Cost_Of? @ 0x00711EC0` — decompiled
  - 7 enum parser helpers — decompiled with tables extracted
  - 4 subclass ReadINI methods (UnitType, InfantryType, BuildingType, AircraftType) — entry and super-call confirmed
  - 4 subclass ctors — super-calls and first-field offsets confirmed
  - TechnoTypeClass vtable @ 0x007F4ED8 — first 32 slots resolved
  - 5 static armor/pipscale/category/vhpscan/veteran tables — bytes read directly
- **Binary string table scan:** `TiberiumImmunity` / `CostLower` / old armor
  tokens confirmed ABSENT.
- **INI files referenced:** `ini/rulesmd.ini`, `ini/artmd.ini` (canonical YR
  entries [E1], [GREIN], [ORCA], [NAPOWR] sampled for key coverage).
- **Prior research (cross-referenced, not superseded):**
  [BUILDINGTYPECLASS_FIELDS.csv](BUILDINGTYPECLASS_FIELDS.csv),
  [BUILDINGTYPECLASS_CTOR_DEFAULTS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BUILDINGTYPECLASS_CTOR_DEFAULTS.md),
  [OWNER_BITMASK_TECH_PREREQUISITE_SYSTEM.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OWNER_BITMASK_TECH_PREREQUISITE_SYSTEM.md),
  [ABSTRACTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ABSTRACTCLASS_GHIDRA_REPORT.md),
  [OBJECTCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECTCLASS_GHIDRA_REPORT.md),
  [READINI_FIELD_MAPS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/READINI_FIELD_MAPS.md),
  [COUNTRY_MULTIPLIERS_APPLICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COUNTRY_MULTIPLIERS_APPLICATION.md).
- **Investigation plan:** `docs/plans/2026-04-24-technotypeclass-base-investigation-plan.md`
  (executed in 3 phases: 6 Phase-1 FULL/MEDIUM fns + Phase-1 checkpoint + 11
  Phase-2 fns + 5 Phase-3 items + static table reads).

No Ghidra functions were renamed during this investigation (per CLAUDE.md's
~90% confidence rule). No `.rs` files were modified. No Rust code was written.
