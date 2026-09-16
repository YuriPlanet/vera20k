# Active-retail Full_Init and RMG-preview native-ID prefix — reinvestigation report

Date: 2026-08-31

Binary: active retail Yuri's Revenge `gamemd.exe`, image base `0x00400000`

Mode: fresh `ScenarioClass::Full_Init` load from `Clear_Scene` through the
`Read_Map_Section_And_IsoMapPacks` snapshot/reservation and first authored Overlay,
plus the contradictory RMG preview setup prefix needed to reconcile OQ-33/OQ-34

System ownership hypotheses: GSI-01.12 native identity/lifetime, GSI-04.12 /
GSI-04.13 generated terrain animation, GSI-04.15 negative boundary, transaction-3
authored Overlay/Anim construction, transaction-5 explicit Tubes

Status: **native prefix and preview branch matrix verified; Rust implementation remains open**

Investigation mode: **exhaustive-slice**, extended when the Cell constructor and
shared deferred-finalization contradictions appeared

**Address(es):** `0x006851F0`, `0x00686B20`, `0x004ACE70`, `0x00599650`

**Investigation Mode:** exhaustive-slice

**Claimed Scope:** fresh active-retail Full_Init native-ID events from the final
Clear_Scene seed through the pre-map snapshot/reservation, Tubes, and first
Overlay/child; matching and rebuilding RMG-preview setup prefixes; fresh shared
deferred-queue prestate; preview tiberium-queue lifetime

**Non-Scope:** save restore, editor writing, runtime gameplay constructors after
the first Overlay/child boundary, and heap-failure emulation beyond the verified
native ID/fault boundary

**Confidence:** High

**Active in YR:** Yes; theater allocation count and child creation are data-conditional

## 1. Overview

There is no correct fixed `1,010,000` seed for the first authored Overlay. A fresh
active-retail `Full_Init` calls `Clear_Scene @ 0x006851F0`, whose final effective
write seeds `ScenarioClass+0x214` to `1,000,000`. Before
`Read_Map_Section_And_IsoMapPacks @ 0x004ACE70` snapshots that field at
`0x004AD026`, actual ID-bearing TypeClass, HouseClass, per-House SuperClass, and
CellClass constructors have already preincremented it. The exact saved value is a
wrap-32 fold of those source-ordered constructor events.

Campaign has one post-reset House/Super pass and one final Cell/dummy pass.
Every active noncampaign fresh load—including ordinary authored skirmish/LAN/WOL
and accepted `.SED` generation—has a disposable first House/Super pass and first
Cell/dummy Resize, then destroys/rebuilds rules and Houses, then performs the same
House/Super and Cell/dummy work a second time. Authored data can add types during
the final map Rules pass; generated synthetic data normally does not. The two paths
share the reservation mechanism but not one absolute numeric constant.

The map reader saves `C_saved`, may perform theater `AnimType` allocations, then
stores **from the snapshot**, not from the then-current cursor:

```text
Scenario+0x214 = wrap32(C_saved + 0x2710)
```

All 176 active `Tile%02dAnim` rows (20 distinct names) in the six retail YR
theater INIs name AnimTypes already present in `rulesmd.ini [Animations]`, so the
between-snapshot allocation count is exactly zero for retail data. Custom theater
data may allocate there; those IDs are real but the later store shadows their
cursor advancement.

Every successfully allocated `[Tubes]` row constructs a Tube and spends one ID
before token parsing. Native has no semantic row-rejection arm. A malformed row can
spend its ID and then fault; allocation failure spends none and then faults. On a
successful load with `T` Tube entries, the first reader-admitted, successfully
allocated Overlay receives:

```text
O1 = wrap32(C_saved + 10_000 + T + 1)
```

Its synchronous child Anim IDs follow the actual Mark trace; for an ordinary row
that creates both a CellAnim and one terrain-tile Anim, the order is
`Overlay O1`, `CellAnim O1+1`, `terrain Anim O1+2`, then the next Overlay.

The fresh prefix contribution to the shared deferred-finalization queue is exactly
empty. `Clear_Scene` zeros/drains the queue; Type, House/Super, Cell, TagType, and
Tube prefix objects do not call any queue writer. Disposable Houses/Supers and old
types are removed synchronously from their registries, and Cell reconstruction
overwrites Cell IDs without joining the shared queue. The queue at the common
`ReadMapOverlayPacks` drain can contain reader-produced Overlay entries, but it has
no inherited prefix entry. The drain remains a shared lifecycle primitive, not an
Overlay-only abstraction.

RMG preview is branch-specific. Every Generate frees old tiberium **spread then
growth** queues, then preview `Set_Defaults` resets the numeric ID cursor. An exact
four-field storage match skips Resize, type reconstruction, House reconstruction,
and theater reload; the first new Building/Anim can receive `1,000,001`. Missing or
changed storage constructs every real Size-diamond Cell in row-major order plus the
dummy Cell, rebuilds ID-bearing types, constructs Houses/Supers, and only then
reaches generator objects. The previous report's unconditional first-object claim
was wrong and is corrected alongside this report.

On an exact match, retained Types, Houses, Supers, real/dummy Cells, old Anims, and
other untouched Abstracts keep their prior numeric IDs while the Scenario counter
resets. Native IDs are therefore globally non-unique in this window. After a prior
successful rebuild, the retained first real Cell normally already owns `1,000,001`,
so the first new constructor deterministically reuses that number. Collision-free
Rust runtime handles must be independent for every live class, not merely Anims.

### Scope, exclusions, and definitions

Included:

1. all direct `AssignUniqueID` constructor families and the subset reachable before
   the `0x004AD026` snapshot;
2. exact order/count inputs for rules/type, House/Super, and Cell construction in
   campaign, authored noncampaign, and generated noncampaign Full_Init;
3. the snapshot, theater allocation window, wrapping reservation, Tubes, first
   Overlay, and child-Anim identity transform;
4. successful, malformed/rejected-policy, and allocation-failure Tube effects;
5. the fresh shared deferred-queue/registry prestate;
6. matching versus missing/changed RMG preview Cell/Type/House prefixes;
7. retained-ID collision scope and preview tiberium-queue lifetime;
8. retail data activation and current Rust ownership boundaries.

Excluded:

- save-game restore, which restores serialized identity and does not run fresh
  Full_Init construction;
- editor-only map writing and dormant TS-only object families;
- runtime gameplay constructors after map load except where needed to delimit the
  first Overlay/child IDs;
- heap-failure emulation beyond its exact native ID/fault boundary;
- Rust edits, Cargo execution, and Ghidra metadata mutation.

OpenTS was used only as a navigation lead. Its `display.cpp` suggested the saved
counter reservation, `scenario.cpp` the preincrement primitive, and `map.cpp` the
Cell reconstruction corridor. Every material conclusion was rechecked in active
`gamemd.exe` and YR retail data.

Notation:

- `a ⊞ b` means 32-bit two's-complement wrapping addition.
- `R(W,H) = H * (2W - 1) + 1` is one successful ordinary Resize's ID cost: all
  real Size-diamond Cells plus the shared dummy Cell.
- `HB(H,S) = H * (1 + S)` is `H` successful Houses when each constructs `S`
  successful SuperClass children.
- `|E|` is the number of actual events in an ordered event stream, never the final
  registry length or the number of source rows.

### Bounded inventory and coverage summary

| Mechanism | Active-retail result | Closure |
|---|---|---|
| Clear_Scene seed | final effective `Scenario+0x214=1,000,000` | RESOLVED |
| NextUniqueID | preincrementing wrap-32 dword sequence | RESOLVED |
| direct Assign caller census | complete direct-xref family list checked | RESOLVED |
| pre-snapshot Type families | 16 ID-bearing families; Particle/Tag/AI definitions excluded | RESOLVED |
| rules source order | explicit registries then fixed lazy-reader order, first-new-name only | RESOLVED |
| retail explicit-list count | 1,704 rows, 1,699 actual distinct family-local constructors | RESOLVED |
| House cost/order | House ID, then one Super ID per current SuperWeaponType in registry order | RESOLVED |
| campaign Houses | `[Houses]` source order; HouseType-order fallback when empty | RESOLVED |
| noncampaign Houses | two session-roster passes around reset | RESOLVED |
| Cell cost/order | row-major real Size-diamond Cells, dummy last; every re-ctor spends | RESOLVED |
| authored/generated difference | same noncampaign skeleton; map type events/dimensions are source-controlled | RESOLVED |
| snapshot/reservation | Set from saved value `+0x2710`, not add current | RESOLVED |
| post-snapshot theater types | possible custom AnimType events, shadowed by saved-value store | RESOLVED |
| retail theater K | exactly zero new AnimTypes | RESOLVED |
| Tubes | every allocated row spends before parse; no reject-and-continue arm | RESOLVED |
| first Overlay formula | `C_saved ⊞ 10000 ⊞ T ⊞ 1` | RESOLVED |
| child Anim order | synchronous Mark order before next Overlay | RESOLVED |
| fresh deferred queue | empty on reader entry; no prefix-produced entries | RESOLVED |
| disposable registry cleanup | synchronous, no dead/duplicate prefix pointers left queued | RESOLVED |
| preview matching prefix | no new prefix constructors; first new ID can be 1,000,001 | RESOLVED |
| preview missing/changed prefix | Cells/dummy, types, Houses/Supers, retail K=0 | RESOLVED |
| preview duplicate window | all retained Abstract-class families, not only Anims | RESOLVED |
| preview tiberium queues | free spread/growth before branch; rebuild growth/spread; persist across Cancel | RESOLVED |
| Rust owner/contract | implementation changes and fixtures specified below | OPEN FOR IMPLEMENTATION |

## 2. Class Layout / Key Offsets

| Owner | Offset / global | Type | Verified purpose |
|---|---:|---|---|
| ScenarioClass | `+0x214` | `u32`/dword | preincrementing native numeric-ID cursor |
| AbstractClass | `+0x10` | `u32`/dword | assigned native numeric ID |
| ScenarioClass | `+0x218` | RNG state | separate Scenario RNG; not reset by the preview ID argument |
| MapSeedClass | `+0x178` | pointer alias | dialog storage snapshot used by the four-field preview branch |
| shared deferred queue | `0x00B0F69C` | pointer array | shared deferred-finalization entries |
| shared deferred queue | `0x00B0F6A8` | dword count | number of queued entries |
| CellClass | `+0x10` | inherited dword | Cell native ID, preserved by exact-match payload reset |
| CellClass | `+0x140` | flags | terrain-Anim latch retained on exact-match preview |

## 3. Core Logic

### 3.1 Counter primitive and complete direct constructor census

#### 3.1.1 The effective fresh seed

`ScenarioClass::Full_Init @ 0x00686B20` calls `Clear_Scene` at `0x00686B65`.
`Clear_Scene @ 0x006851F0` writes `1,000,000` near entry, calls
`ScenarioClass::Set_Defaults @ 0x00683610` (which also writes that value at
`0x00683633`), performs scene teardown, and writes `1,000,000` again at
`0x00685659`. The tail write is the effective seed for every later constructor in
this fresh Full_Init. Any ID consumed inside teardown is deliberately erased by
that final write.

`ScenarioClass::NextUniqueID @ 0x0068BCB0` performs one 32-bit increment of
`Scenario+0x214`. `AbstractClass::AssignUniqueID @ 0x00410230` calls it and stores
the returned dword at `AbstractClass+0x10`. It performs no collision lookup,
registry query, saturation, or refund.

#### 3.1.2 Direct AssignUniqueID xrefs

A cold direct-xref enumeration found these constructor families:

| Family | Constructor |
|---|---|
| active object instances | Aircraft `0x00413D20`, Anim `0x00421EA0`, Building `0x0043B740`, Bullet `0x00466380`, EMPulse `0x004C52B0`, Factory `0x004C98B0`, Infantry `0x00517A50`, Overlay `0x005FC380`, Particle `0x0062B5E0`, ParticleSystem `0x0062DC50`, Smudge `0x006B4A50`, Terrain `0x0071BB90`, Unit `0x007353C0`, VeinholeMonster `0x0074C5B0`, VoxelAnim `0x007493B0/0x007498D0`, Wave `0x0075E950` |
| direct scenario structures | Cell `0x0047BBF0`, House `0x004F54A0`, Super `0x006CAF90`, Tube `0x00727FD0` |
| ID-bearing types | AircraftType `0x0041C8B0`, AnimType `0x00427530`, BuildingType `0x0045DD90`, BulletType `0x0046BBC0`, HouseType `0x005113F0`, InfantryType `0x005236A0`, OverlayType `0x005FE250`, ParticleSystemType `0x006440A0`, Side `0x006A4550`, SmudgeType `0x006B5260`, SuperWeaponType `0x006CE5B0`, TerrainType `0x0071DA80`, UnitType `0x007470D0`, VoxelAnimType `0x0074AD80`, WarheadType `0x0075CEC0`, WeaponType `0x00771C70` |

Before the `0x004AD026` snapshot, only the 16 ID-bearing type families plus
House, Super, and Cell are reachable on a successful fresh load. Tube is after the
reservation. Overlay/Anim/Terrain/Smudge/Techno instances load later. No other
direct Assign caller can reach the snapshot corridor.

Notably excluded:

- `ParticleTypeClass` never calls AssignUniqueID;
- ScriptType, TeamType, TaskForce, TriggerType, TagType, and AITriggerType use
  `AbstractTypeClass::Constructor @ 0x00410800`, which does not call Assign;
- IsometricTileType and TiberiumClass constructors do not call Assign;
- helpers such as `FUN_00689880` only copy values and consume no ID.

### 3.2 Rules/type constructor stream

#### 3.2.1 Explicit registry order

`RulesClass::Process @ 0x00668BF0` performs its ID-bearing explicit registry work
in this order. A row constructs only when its value is a new case-insensitive name
within that family; repeated names cost zero:

```text
Countries -> HouseType
Sides -> Side
OverlayTypes -> OverlayType
SuperWeaponTypes -> SuperWeaponType
Warheads -> WarheadType
SmudgeTypes -> SmudgeType
TerrainTypes -> TerrainType
BuildingTypes -> BuildingType
VehicleTypes -> UnitType
AircraftTypes -> AircraftType
InfantryTypes -> InfantryType
Animations -> AnimType
VoxelAnims -> VoxelAnimType
Particles -> ParticleType                 // no native ID
ParticleSystems -> ParticleSystemType
```

Each section is visited in INI source-entry order. Later Rules passes preserve the
registries: an existing name updates fields but emits no constructor/ID event; a
new name appends and emits exactly one.

Retail `rulesmd.ini` contains the following active rows:

| Section | rows | distinct family-local names / ID events |
|---|---:|---:|
| Countries | 14 | 14 |
| Sides | 5 | 5 |
| OverlayTypes | 250 | 250 |
| SuperWeaponTypes | 12 | 12 |
| Warheads | 105 | 105 |
| SmudgeTypes | 46 | 46 |
| TerrainTypes | 78 | 78 |
| BuildingTypes | 403 | 402 (`NAPSYA` repeats) |
| VehicleTypes | 80 | 80 |
| AircraftTypes | 12 | 12 |
| InfantryTypes | 65 | 65 |
| Animations | 611 | 607 (`GAWEAP_1`, `GAWEAP_2`, `GAWEAP_A`, `TWLT100` repeat) |
| VoxelAnims | 10 | 10 |
| ParticleSystems | 13 | 13 |

That is 1,704 ID-bearing source rows but exactly **1,699 explicit constructor
events** on an empty family registry. `Particles` has 22 rows and is deliberately
outside both totals because its constructor has no Assign call.

The 1,699 count is not the complete Rules prefix. It is an exact retail explicit-
list subtotal, not an Overlay seed.

#### 3.2.2 Lazy allocations and exact trace rule

After explicit lists, fixed readers can FindOrAllocate names referenced by General,
type bodies, weapons, projectiles, warheads, particles, crate/special-weapon data,
and later Rules helpers. WeaponType and BulletType have no numbered master lists,
so their referenced first-new-name events are material. Other families can also
gain names absent from their explicit lists.

`RulesClass__ReadTypeData @ 0x00679A10` visits live registries in this fixed order:

```text
HouseType -> SuperWeaponType -> AnimType (fixed ART INI)
-> BuildingType -> AircraftType -> UnitType -> InfantryType
-> WeaponType -> BulletType -> WarheadType
-> weapon post -> building post
-> TerrainType -> SmudgeType -> OverlayType
-> ParticleType -> ParticleSystemType -> VoxelAnimType -> MissionControl
```

Within a reader, existing live type-array order and the reader's fixed key order
determine referenced-name allocation order. Therefore the exact contract is an
ordered `FindOrAllocate` event stream, not `sum(final registry lengths)`. Deleting
types during reset does not refund their IDs; rebuilding them constructs them
again and spends again.

The active reload stack is:

1. base rules;
2. optional language rules when present;
3. current nonzero-mode INI where applicable;
4. the map/synthetic INI `RulesClass::Process` call in Full_Init.

`P` below means every actual ID-bearing constructor event across that ordered stack,
including lazy allocations. It is exactly computable from the same source layers,
but it is not one retail-wide constant because mode/map/language content can add
names.

### 3.3 House and Super constructor blocks

`HouseClass::Constructor @ 0x004F54A0` assigns the House ID at `0x004F5E4E`,
joins its immediate registries, and then walks the current
`g_SuperWeaponTypeClass_Array` in registry order. Every successful `0x80` allocation
calls `SuperClass::Constructor @ 0x006CAF90`, which assigns one ID at `0x006CB011`.

Normal successful-heap order is therefore:

```text
House[i]
  Super[i,0]
  Super[i,1]
  ...
  Super[i,S-1]
House[i+1]
```

and the cost is `HB(H,S)=H*(1+S)`. A nested Super allocation failure skips that
Super constructor/ID and leaves a null slot; callers that cannot allocate the
House itself do not have a normal successful continuation. The Rust load policy may
hard-error those cases instead of emulating partial native registry degradation.

House source order differs by load family:

- `ScenarioClass__Create_Houses @ 0x00687F10` constructs session human nodes in
  stable ascending signed node `+0x53` order (source index breaks ties), then valid
  computer slots in ascending slot order, then Neutral and Special.
- campaign `FUN_005009B0` constructs one House for every `[Houses]` row in source
  order. If that section constructs none, it falls back to every current HouseType
  in registry order.

For noncampaign Full_Init, `Create_Houses` runs once before the rules reset and
again through `ScenarioClass__Read_INI_Basic @ 0x00689E90` after the reset. The
first Houses and their Supers are scalar-deleted synchronously during reset. Their
IDs remain spent; their registry pointers and listener entries are removed rather
than queued.

### 3.4 CellClass Resize prefix

`Read_Map_Section_And_IsoMapPacks` calls the Map Resize vslot at `0x004ACF0D`
before the snapshot. `MapClass::Resize @ 0x00565C10` visits the full `[Map] Size`
diamond in row-major storage order. The admission inequalities produce exactly
`H*(2W-1)` real Cells for positive width `W` and height `H`.

For every admitted slot:

- null storage allocates `0x148` and calls `CellClass::Constructor @ 0x0047BBF0`
  at `0x005663D6`;
- existing storage calls the same constructor in place at `0x005663FC`;
- the shared dummy Cell is reconstructed unconditionally at `0x005670F2` after
  the real loop.

The Cell constructor calls `AbstractClass::Constructor_Full`, initializes fields
and both embedded arrays, then calls AssignUniqueID at `0x0047BD8F/0x0047BD90`.
It does not join ObjectClass or the shared deferred queue. Reconstructing an
existing Cell overwrites its old `+0x10` numeric ID; the old value is no longer live
but remains spent in the Scenario counter. The dummy behaves the same way.

Thus each successful ordinary Resize costs exactly:

```text
R(W,H) = H*(2W-1) + 1
```

An allocation failure skips that Cell constructor/ID and is followed by a native
null dereference; it cannot lead to a successful first-Overlay oracle.

### 3.5 Exact source-dependent Full_Init formulas

#### 3.5.1 Event definitions

Let:

- `E_campaign` be actual new ID-bearing types created by the optional early
  campaign companion/sidecar Rules pass before the later reset;
- `E_multi` be actual new ID-bearing types created by the noncampaign early
  Countries -> General -> live HouseType-body prepass against the process's
  current pre-reset registries;
- `H1,S0,R1` describe the disposable noncampaign House/Super and Resize pass;
- `P` be the complete ordered post-reset base/language/mode/map type event stream;
- `H2,S1,R2` describe the final House/Super and final map Resize pass;
- `Hc` be campaign `[Houses]`/fallback House count.

These symbols are actual successful constructor event counts. They are source and
prestate inputs, not estimates.

The active-stock cold owner entering the first noncampaign prepass is now closed by
[LOAD_GAME_RULES_COLD_START_NATIVE_REGISTRY_PRESTATE_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/LOAD_GAME_RULES_COLD_START_NATIVE_REGISTRY_PRESTATE_REINVESTIGATION_GHIDRA_REPORT.md):
startup produces 1,070 ID-bearing Type events and retained registry state, but its
event vector predates the Scenario cursor reset and is not part of `E_multi`. Against
that retained state, Countries -> General -> live HouseType bodies emits exactly 51
stock `E_multi` events (E-only hash `0x45b8b69cd005937d`) before Create_Houses.

#### 3.5.2 Campaign

Campaign has no noncampaign early House/Resize pass. The exact snapshot is:

```text
C_saved_campaign = 1_000_000
                   ⊞ |E_campaign|
                   ⊞ |P|
                   ⊞ HB(Hc,S1)
                   ⊞ R2
```

Order is early optional type events, reset/reload/map type events, campaign
House/Super blocks, final row-major Cells, dummy, snapshot.

#### 3.5.3 Authored noncampaign

The exact snapshot for skirmish/LAN/WOL-style fresh Full_Init is:

```text
C_saved_noncampaign = 1_000_000
                       ⊞ |E_multi|
                       ⊞ HB(H1,S0)
                       ⊞ R1
                       ⊞ |P|
                       ⊞ HB(H2,S1)
                       ⊞ R2
```

On ordinary stock success, the two session rosters have the same count and the two
Resizes use the same map dimensions, but the formula retains separate terms because
the pre-reset and post-reset SuperWeapon registries are distinct inputs. Authored
map Rules can append type events inside `P`.

#### 3.5.4 Accepted `.SED` generated launch

Argument-zero synthetic generation reaches the same noncampaign Full_Init. It is
not a reduced prefix:

```text
C_saved_SED = C_saved_noncampaign
```

with the `.SED` Size/session inputs and its actual Rules event stream. The generated
synthetic map normally adds zero map-specific Type constructors, whereas an authored
map may add them. Both still perform two House/Super passes and two Cell/dummy
Resizes. This answers the design-gate question: **the `C_saved+10,000` reservation
belongs to every fresh Full_Init authored load as well as `.SED` launch.**

#### 3.5.5 Why an absolute constant would be false

Even with retail base data, these values vary with:

- campaign versus noncampaign structure;
- current session House roster;
- current/pre-reset versus reloaded SuperWeaponType counts;
- `[Map] Size`;
- language/mode/map Rules additions and lazy referenced names;
- process prestate for the early delta.

The exact deliverable is therefore a consumed-once constructor trace and the above
fold, not a hard-coded retail number.

### 3.6 Snapshot, reservation, Tubes, and first Overlay

#### 3.6.1 Set-from-snapshot semantics

Inside `Read_Map_Section_And_IsoMapPacks`:

```text
004AD026  MOV ESI,[Scenario+0x214]   // C_saved
...
004AD059  ADD ESI,0x2710
004AD05F  MOV [Scenario+0x214],ESI
```

Between those instructions, a theater-change path can call
`Read_Theater_TileSets_INI @ 0x00545150`. `Tile%02dAnim` handling calls
`AnimTypeClass::FindOrAllocate @ 0x00428B80` at `0x00546538`. A new name therefore
gets a real ID derived from `C_saved`, but the `0x004AD05F` write ignores the
advanced current value and installs `C_saved ⊞ 10,000`.

The trace must preserve:

```text
Snapshot(C_saved)
ShadowedAssignUniqueID*          // zero for retail active theater data
SetCursorFromSnapshotPlus(0x2710)
```

It must not model this as `counter += 10,000`.

The add is raw x86 dword arithmetic. Example:
`0xFFFFFFF0 ⊞ 0x2710 = 0x00002700`.

#### 3.6.2 Retail K_theater is zero

The six active retail YR theater files (`temperatmd`, `snowmd`, `urbanmd`,
`urbannmd`, `desertmd`, `lunarmd`) contain 176 active `Tile%02dAnim` rows and 20
distinct values:

```text
TUNTOP01..TUNTOP04
WA01X..WA04X
WB01X..WB04X
WC01X..WC04X
WD01X..WD04X
```

Every one is already a case-insensitive member of retail
`rulesmd.ini [Animations]`. Therefore `FindOrAllocate` returns an existing
AnimType for every active retail row and `K_theater=0`. This is a retail-data
outcome, not a reason to erase the shadowed-allocation trace arm for custom data.

#### 3.6.3 Non-ID work before Tubes/Overlay

Map CellTags can FindOrAllocate `TagTypeClass` through `FUN_006E6310`, but TagType
does not call AssignUniqueID. `FUN_00465CC0` only resolves BuildingType `ToTile`;
`FUN_004F42F0` only flags tactical state/increments the bridge counter. Neither
changes the native ID cursor or the shared deferred queue.

#### 3.6.4 `[Tubes]`

`MapClass::ReadTubesINI @ 0x007283C0` visits every `[Tubes]` entry in source/index
order. For each row it reads the string, allocates `0x1C4`, and on allocation
success calls `TubeClass::Constructor @ 0x00727FD0`, whose Assign call is at
`0x00728017`. Only after construction does tokenization/`atoi` populate fields.

Exact outcomes:

| Input/heap event | ID effect | Continuation |
|---|---:|---|
| no `[Tubes]` section / zero rows | 0 | Overlay reader continues |
| allocated well-formed row | +1 | row completes |
| allocated malformed row | +1 before parse | may fault; no native reject-and-continue branch |
| allocation failure | 0 | native dereferences null and faults |

Consequently, every successful load has `T = [Tubes] entry count`, not “validated
Tube count.” A Rust safe malformed-row policy may hard-error, but a reject-and-
continue policy that spends zero would invent behavior.

#### 3.6.5 First Overlay and child IDs

Let `B = C_saved ⊞ 10,000`. After `T` successful Tube constructors, the first
Overlay row that passes format/identity/image-or-CellAnim/multiplayer-crate/radar
admission and whose `0xB0` allocation succeeds receives:

```text
O1 = B ⊞ T ⊞ 1
```

A pre-construction reject or Overlay allocation failure spends no ID. The Overlay
constructor assigns its ID before Mark. Mark completes synchronously before the
next decoded coordinate, so every child Assign event advances the same cursor.

For an ordinary admitted row:

| children constructed in that Mark | IDs |
|---|---|
| none | next Overlay starts after `O1` |
| CellAnim only | CellAnim `O1⊞1` |
| terrain tile Anim only | terrain Anim `O1⊞1` |
| CellAnim then terrain tile Anim | CellAnim `O1⊞1`, terrain Anim `O1⊞2` |

Low/high structural Mark can touch more than the receiver cell; the general rule is
to preserve every synchronous child Assign event in actual Mark order before the
next Overlay, not to force a fixed two-child count.

### 3.7 Shared deferred-finalization queue and registry prestate

#### 3.7.1 Queue entry state is exactly empty

The shared queue uses data `0x00B0F69C` and count `0x00B0F6A8`.
`FUN_00534450`, called by `Clear_Scene`, writes the count to zero at `0x00534465`,
deletes each scene registry, and calls
`DrainDeferredFinalizationQueue @ 0x00725C70` between families and after the final
map/tactical cleanup. `Clear_Scene` then returns to Full_Init with no queued entry.

A complete xref pass over the queue count/data found writers only in:

- ObjectClass UnInit/destructor paths;
- TagClass event/destructor helpers;
- DiskLaser helpers;
- ParticleSystem instance construction/AI;
- BulletAnimTracker;
- the generic QueueForDeferredFinalization helper;
- the drain itself and Clear_Scene's reset.

No fresh pre-reader constructor/destructor reaches one of those writers:

- ID-bearing Type classes are AbstractType-derived, not ObjectClass;
- House and Super are direct Abstract-derived; House destruction synchronously
  destroys Supers, dispatches pointer expiration, and removes registry/listener
  entries before `AbstractClass::Destructor_ResetVtables`;
- Cell is direct Abstract-derived and is reconstructed in place without an Object
  queue join;
- Script/Team/TaskForce/TriggerType/TagType/AITriggerType definitions are
  AbstractType-derived;
- Tube is direct Abstract-derived and appears only after reservation;
- no actual Techno/Anim/Overlay/Terrain/Smudge/Particle instance is constructed
  before the reader on fresh Full_Init.

Thus:

```text
Q_on_ReadMapOverlayPacks_entry = []
```

There is no alive prefix entry, dead prefix entry, duplicate prefix pointer, or
prefix-created deferred object.

#### 3.7.2 Registry cleanup details

The disposable first-pass Houses/Supers do temporarily join their live class and
listener registries. The rules reset invokes their scalar destructors, which remove
those pointers synchronously; the second pass creates new objects once. Old
ID-bearing Type registries are likewise destroyed before reconstruction. Their IDs
are not refunded, but no dead pointer is left for the later drain.

First-pass Cells are not deferred objects. The second Resize's in-place Cell
constructor overwrites each live numeric ID; it does not append another pointer to
an Object registry. The dummy is similarly overwritten.

#### 3.7.3 State at the common drain

`ReadMapOverlayPacks @ 0x005FD2E0` always calls the shared drain at its common tail
near `0x005FD692`, even when one or both pack bodies are absent. Starting from the
proved empty entry state, the identity pass can append its own dead/uninitialized
Overlay pointers in decoded order; steep-slope survivors remain alive and unqueued.

Therefore the pre-drain queue at `0x005FD692` is:

```text
Q_before_common_drain = reader-produced Overlay lifecycle entries only
```

on a fresh load—not because the queue is Overlay-specific, but because the shared
prefix seed is empty. A no-identity/empty pack reaches the same drain with `[]`.

### 3.8 RMG preview reconciliation

#### 3.8.1 Work before the branch

`RandomMapGenerator__InitMapFromSyntheticINI @ 0x00599650` unconditionally calls:

```text
0x00599A13  TiberiumClass__FreeSpreadQueues_All
0x00599A18  TiberiumClass__FreeGrowthQueues_All
```

before the preview/launch argument split. The preview branch then calls
`ScenarioClass::Set_Defaults` at `0x00599B23`, resetting the numeric cursor to
`1,000,000`.

Queue freeing therefore precedes both the ID reset and the storage-key cleanup
decision.

#### 3.8.2 Exact-match storage

When the snapshot exists and all four normalized key fields match, `bVar3=false`:

- full cleanup is skipped;
- Resize is skipped;
- rules/type reconstruction and `FUN_00689880` are skipped;
- House reconstruction and theater tile-set reload are skipped;
- selective loops delete Unit, Infantry, Building, and Terrain instances;
- the Cell payload iterator writes `+0x11C,+0x11B,+0x38,+0x11A,+0x44,+0x11E`
  but not `AbstractClass+0x10` and not the dummy Cell.

No actual AssignUniqueID constructor intervenes between the reset and the first
generator Building/Anim. Therefore:

```text
C_preview_match_before_generator = 1_000_000
first new generator object ID     = 1_000_001
```

This value can be a duplicate. Retained HouseClass, each SuperClass, every
ID-bearing TypeClass, every real Cell, the dummy Cell, old AnimClass instances, and
any other untouched Abstract keep their previous `+0x10`. AssignUniqueID does not
search them.

After a prior missing/changed preview successfully built the same storage, its
first real Cell received `1,000,001`. The next exact-match Generate resets the
counter but retains that Cell, so any first new Building/Anim also receives
`1,000,001` while the Cell remains live. Old Anim overlap is an additional later
possibility, not the sole duplicate window.

#### 3.8.3 Missing or changed storage

Missing storage or any changed key keeps `bVar3=true`. The branch performs full
cleanup, then Resize, rules/type rebuild, House fallback, and theater work in this
order:

```text
Seed(1_000_000)
-> R(W,H) Cell/dummy Assign events
-> P_preview ID-bearing type Assign events
-> H_preview House blocks, each with S_preview Supers
-> K_preview theater AnimType Assign events
-> first generator Building/Anim
```

`FUN_00689880` copies up to 50 `[VariableNames]` strings and consumes no ID.
If the House array is empty—as it is after ordinary full cleanup—the loop constructs
one House per current HouseType in registry order, and each House constructs every
current SuperWeaponType. The exact successful cursor is:

```text
C_preview_rebuild = 1_000_000
                    ⊞ R(W,H)
                    ⊞ |P_preview|
                    ⊞ HB(H_preview,S_preview)
                    ⊞ K_preview

first generator ID = C_preview_rebuild ⊞ 1
```

For active retail data, `K_preview=0` by the same 176-row/20-name proof in
section 6.2. Custom theater data retains the event arm.

#### 3.8.4 Queue lifetime through Cancel, re-entry, replacement, and launch

`RandomMapGenerator__Generate @ 0x00598960` later rebuilds growth queues at
`0x0059939B`, then spread queues at `0x005993A0`, from the then-current generated
map state. These final queues persist across common dialog teardown, Cancel, and a
no-Generate re-entry; those boundaries do not call either free routine.

The next Generate, regardless of matching versus changed storage, frees old spread
then growth queues at `0x00599A13/0x00599A18` and later rebuilds growth then spread.
Argument-zero `.SED` launch takes this same initial free pair and then Full_Init's
`Clear_Scene`, which again frees spread at `0x00685515` and growth at `0x0068551A`;
Full_Init rebuilds growth at `0x00687A85` and spread at `0x00687A8A`.

The process-shell preview lifecycle must therefore retain the final queue state
alongside live objects/latches/sounds/counter across Cancel and re-entry, while its
next-generation transaction owns the exact free/rebuild order.

## 4. INI Keys

| Section / key | Type / default | Native-ID effect | Active-retail evidence |
|---|---|---|---|
| Rules master lists (`Countries` through `ParticleSystems`) | ordered `index=name` rows; absent section adds none | first new family-local name constructs its Type in fixed family/source order; `Particles` constructs no ID-bearing type | `rulesmd.ini`; `RulesClass::Process @ 0x00668BF0` |
| referenced type-body keys | case-insensitive type names; key-specific defaults | a first referenced absent name can lazily construct Weapon/Bullet/other ID-bearing types in reader/key order | `RulesClass__ReadTypeData @ 0x00679A10` |
| campaign `[Houses]` | ordered House rows; empty invokes HouseType-order fallback | one House then every current SuperWeaponType per admitted row/fallback entry | campaign reader `0x005009B0` |
| `[Map] Size` | `x,y,width,height`; successful fresh load requires usable positive dimensions | every admitted diamond Cell row-major plus dummy: `R(W,H)=H*(2W-1)+1` per Resize | map reader `0x004ACE70`; Resize `0x00565C10` |
| `[Tubes]` entries | token strings; absent/empty costs zero | every successfully allocated source row constructs/spends before parsing; malformed allocated rows spend then can fault | `ReadTubesINI @ 0x007283C0` |
| theater `Tile%02dAnim` | AnimType name; absent row adds none | absent type can allocate after snapshot but before the saved-value reservation store | six YR theater INIs; lookup `0x00546538` |
| synthetic preview rules/map fields | generated INI values | missing/changed branch feeds `P_preview`, dimensions, Houses/Supers, and theater arm; exact match skips those constructors | `InitMapFromSyntheticINI @ 0x00599650` |

No INI key directly specifies the Scenario native-ID cursor, the `+0x2710`
reservation, or queue lifetime; those are hard-coded engine lifecycle effects.

## 5. Integration Points

| Integration | Caller / callee | Timing and verified effect |
|---|---|---|
| fresh scenario entry | `Full_Init @ 0x00686B20` -> `Clear_Scene @ 0x006851F0` | tears down prior scene and leaves the effective `1,000,000` seed |
| rules/type prefix | Full_Init -> reset/reload -> `RulesClass::Process` / `ReadTypeData` | emits source-ordered Type Assign events before final Houses/Cells |
| noncampaign disposable prefix | Full_Init -> `Create_Houses @ 0x00687F10` and Resize | first House/Super and Cell/dummy generation remains spent through later reset/reconstruction |
| final House prefix | `Read_INI_Basic @ 0x00689E90` or campaign House reader | House Assign precedes per-House Supers; source order depends on load family |
| map snapshot/reservation | `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70` | final Resize, snapshot at `0x004AD026`, optional theater types, set-from-snapshot `+0x2710` |
| post-reservation map owners | map reader -> CellTags/Tubes/Overlay packs | TagType costs zero; Tubes consume once; Overlay/children continue the same cursor |
| shared lifecycle | `ReadMapOverlayPacks @ 0x005FD2E0` -> drain `0x00725C70` | fresh entry queue is empty; reader-generated entries reach the shared common drain |
| preview Generate | `Generate @ 0x00598960` -> `InitMapFromSyntheticINI @ 0x00599650` | free spread/growth, reset, choose exact-match or rebuild prefix, generate, rebuild growth/spread |
| dialog Cancel/re-entry | `RandomMapSetupDialog__Run @ 0x00595BC0` | destroys presentation/snapshot only; live native objects/IDs/sounds/queues persist |
| accepted `.SED` launch | synthetic argument zero -> Full_Init | repeats initial queue frees, Clear_Scene teardown/frees, noncampaign prefix, reservation, then queue rebuild |

## 6. Current Rust Implementation Status

### 6.1 Current useful owners and gaps

`src/rules/ini_parser.rs::RulesPassProcessor` already preserves source-ordered
rules passes, family-local FindOrAllocate behavior, and most lazy reference timing.
It is the best existing owner to *emit* an ID-bearing Type constructor trace, but
its current compatibility projection discards constructor events, includes
ParticleType among registry families even though it has no native ID, and handles
Sides outside the listed ID-family projection. Final registry lengths cannot
reconstruct the prefix after the fact.

`src/sim/scenario_bootstrap.rs::PreFillScenarioPrefixPlan` currently owns the two
House RNG passes but no independent native-ID event stream. It should carry, or be
paired one-for-one with, the consumed-once native prefix derived from the same
campaign/session/Size/rules inputs.

`src/sim/world/substrate.rs::ObjectSubstrate` owns collision-free `u64` stable
handles. Preserve that allocator. `src/sim/anim_class.rs` currently copies a stable
handle into `native_unique_id`; that is invalid for both the fresh launch prefix and
preview reset/retention duplicates.

### 6.2 Required trace shape

Names may differ, but the owner must preserve these semantic events:

```rust
enum NativeIdPrefixEvent {
    Seed { value: u32 },
    Assign { class: NativeIdClass, source: NativeIdSource },
    DestroyWithoutRefund { handle: RuntimeHandle },
    SnapshotPreMap { value: u32 },
    ShadowedAssign { class: NativeIdClass, source: NativeIdSource },
    SetCursorFromSnapshotPlus { addend: u32 },
    TubeAssign { source_entry_ordinal: usize },
}
```

Required properties:

1. one wrapping `u32`/`i32` numeric cursor, preincremented exactly once per actual
   Assign event;
2. exact class/source order for rules, Houses/Supers, real Cells, and dummy Cell;
3. campaign/noncampaign structure selected before construction;
4. one snapshot event and a set-from-snapshot reservation event;
5. shadowed post-snapshot Type events retained even though retail count is zero;
6. one Tube binding per successfully constructed source row, consumed exactly once
   by transaction 5 without a second allocation;
7. first Overlay/child Anim consumption continues the same cursor;
8. runtime handles remain collision-free and independent of the reproduced native
   number for **every** live class;
9. fresh reader entry asserts an empty shared deferred queue, then uses the existing
   shared queue/drain rather than an Overlay-private collection;
10. preview state owns retained Abstract IDs and tiberium queues across dialog
    teardown and applies free/reset/rebuild in native order.

### 6.3 Design corrections

1. Replace any `first Overlay = 1,010,001`/`1,010,000` constant with the complete
   source-derived `C_saved` trace.
2. Apply the same reservation to authored fresh Full_Init and `.SED` launch.
3. Replace “valid/rejected Tube rows” counting with “every successfully allocated
   source row”; native has no reject-and-continue arm.
4. Preserve snapshot/shadow/set semantics; do not add 10,000 to the current cursor.
5. Generalize preview native-ID non-uniqueness from old Anims to retained Types,
   Houses/Supers, Cells/dummy, Anims, and every other untouched Abstract.
6. Add preview growth/spread queue lifetime to `PreviewNativeLifecycle` or its exact
   equivalent.
7. Do not seed the common finalization drain as Overlay-only. Assert the fresh
   prefix contribution is empty, then let reader lifecycle events populate the
   shared queue.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Clear_Scene effective seed | verified | `0x006851FC`, `0x00683633`, final `0x00685659` | none |
| NextUniqueID / Assign primitive | verified | `0x0068BCB0`, `0x00410230` | none |
| direct Assign caller census | verified | complete direct xrefs; section 3.1.2 | none in bounded pre-snapshot corridor |
| explicit Rules family order | verified | `RulesClass::Process @ 0x00668BF0`; `rulesmd.ini` | none |
| retail explicit-list subtotal | verified | 1,704 ID-bearing rows; 1,699 distinct constructors; section 3.2.1 | lazy events remain source-derived, not a fixed subtotal |
| lazy Type event order | verified | `ReadTypeData @ 0x00679A10` plus fixed live-registry/key order | implementation must preserve the event trace |
| House/Super constructor order | verified | `0x004F54A0`, `0x006CAF90` | none |
| campaign House source order | verified | campaign reader `0x005009B0` | none |
| noncampaign two House/Resize passes | verified | `Create_Houses @ 0x00687F10`, `Read_INI_Basic @ 0x00689E90`, Resize `0x00565C10` | none |
| Cell and dummy ID cost | verified | ctor `0x0047BBF0`; call sites `0x005663D6`, `0x005663FC`, `0x005670F2` | none |
| campaign Full_Init formula | verified | section 3.5.2 | input-derived trace must be implemented |
| authored noncampaign formula | verified | section 3.5.3 | input-derived trace must be implemented |
| accepted `.SED` formula | verified | section 3.5.4 | same noncampaign skeleton, source-specific inputs |
| snapshot/set-from-snapshot reservation | verified | `0x004AD026..0x004AD05F` | none |
| post-snapshot theater allocations | verified | lookup `0x00546538` | retain custom-data shadowed-event arm |
| active-retail theater count | verified | six theater INIs: 176 rows, 20 distinct, all present in `rulesmd.ini` | none; `K_theater=0` |
| Tubes constructor/parse boundary | verified | `ReadTubesINI @ 0x007283C0`, ctor `0x00727FD0` | safe Rust may hard-error but must not invent reject-and-continue |
| first Overlay/child formula | verified | Overlay ctor `0x005FC380`; synchronous Mark trace | structural multi-cell children must follow actual event trace |
| fresh deferred-queue prestate | verified | queue xrefs, `FUN_00534450`, globals `0x00B0F69C/0x00B0F6A8` | none; entry is `[]` |
| common shared drain | verified | `ReadMapOverlayPacks @ 0x005FD2E0`, drain `0x00725C70` | use shared, not Overlay-private, queue |
| preview exact-match prefix | verified | `InitMapFromSyntheticINI @ 0x00599650`; skipped Resize/helpers | implement zero setup Assigns and retained IDs |
| preview missing/changed prefix | verified | Resize/rules/House/theater call corridor; section 3.8.3 | implement `R+P_preview+HB+K_preview` |
| preview duplicate-ID scope | verified | reset `0x00599B23`; retained Cell payload/registries; unchecked Assign | runtime handles must remain independent for every class |
| preview tiberium queues | verified | frees `0x00599A13/18`; inits `0x0059939B/A0`; launch/clear sites | implement persistence and exact free/rebuild order |
| current Rust ownership/delta | verified | section 6; cited Rust owners | implementation remains open |

## 8. Open Questions — Final State of the Investigation Log

- `[RESOLVED] OQ-01 — Which Clear_Scene reset is effective? → The final tail write leaves 1,000,000.` (evidence: `0x00685659`)
- `[RESOLVED] OQ-02 — Does NextUniqueID return then increment? → No; it preincrements the dword and returns/stores the new value.` (evidence: `0x0068BCB0`, `0x00410230`)
- `[RESOLVED] OQ-03 — Which direct Assign families reach the snapshot? → The 16 ID-bearing Type families plus House, Super, and Cell.` (evidence: direct xrefs summarized in section 3.1.2)
- `[RESOLVED] OQ-04 — Does ParticleType consume an ID? → No.` (evidence: ParticleType constructor and absent Assign xref; `RulesClass::Process @ 0x00668BF0`)
- `[RESOLVED] OQ-05 — Do Tag/Script/Team/Trigger definition constructors consume IDs? → No; they use the non-Assign AbstractType constructor.` (evidence: `AbstractTypeClass::Constructor @ 0x00410800`)
- `[RESOLVED] OQ-06 — Are explicit list rows equal to constructor count? → No; retail has 1,704 ID-bearing rows and 1,699 family-local first-new constructors.` (evidence: `rulesmd.ini`; section 3.2.1)
- `[RESOLVED] OQ-07 — What orders lazy type events? → Live registry order plus fixed reader/key order in each source pass.` (evidence: `ReadTypeData @ 0x00679A10`)
- `[RESOLVED] OQ-08 — Does House assign before Supers? → Yes; one House Assign precedes every SuperWeaponType-order Super Assign.` (evidence: `0x004F54A0`, `0x006CAF90`)
- `[RESOLVED] OQ-09 — Is campaign a two-House-pass load? → No; it has one post-reset Houses/fallback pass.` (evidence: campaign reader `0x005009B0`; Full_Init branch)
- `[RESOLVED] OQ-10 — Are authored and .SED noncampaign fresh loads two-pass? → Yes; both use the disposable and final House/Resize skeleton.` (evidence: `0x00687F10`, `0x00689E90`, `0x00565C10`)
- `[RESOLVED] OQ-11 — Does an existing Cell re-constructor spend an ID? → Yes; it overwrites the old live Cell ID.` (evidence: `0x005663FC`, ctor Assign `0x0047BD8F/90`)
- `[RESOLVED] OQ-12 — Is the dummy Cell included? → Yes; one unconditional final Cell constructor runs per Resize.` (evidence: `0x005670F2`)
- `[RESOLVED] OQ-13 — Is the reservation added to the current cursor? → No; it stores C_saved+0x2710 from the snapshot variable.` (evidence: `0x004AD026..0x004AD05F`)
- `[RESOLVED] OQ-14 — Can theater loading allocate after the snapshot? → Yes; an absent Tile AnimType receives a real shadowed ID before the saved-value store.` (evidence: `0x00546538`)
- `[RESOLVED] OQ-15 — Does active retail allocate there? → No; K_theater=0.` (evidence: 176 active rows / 20 distinct names across six theater INIs, all in `rulesmd.ini [Animations]`)
- `[RESOLVED] OQ-16 — Do malformed Tubes reject before construction? → No; successful allocation constructs and spends before parsing.` (evidence: `ReadTubesINI @ 0x007283C0`, `TubeClass::Constructor @ 0x00727FD0`)
- `[RESOLVED] OQ-17 — Can Tube OOM continue to Overlay? → No; it spends zero and then faults on the native null path.` (evidence: `ReadTubesINI @ 0x007283C0`)
- `[RESOLVED] OQ-18 — Does the fresh prefix seed deferred objects? → No; reader entry queue is exactly empty.` (evidence: queue xrefs, `FUN_00534450`, globals `0x00B0F69C/0x00B0F6A8`)
- `[RESOLVED] OQ-19 — Is the common drain Overlay-specific? → No; it is shared, although only reader-produced Overlay entries can populate this fresh corridor.` (evidence: `ReadMapOverlayPacks @ 0x005FD2E0`, `DrainDeferredFinalizationQueue @ 0x00725C70`)
- `[RESOLVED] OQ-20 — Can matching-preview first object get 1,000,001? → Yes; no setup Assign intervenes, and retained cross-class IDs can duplicate it.` (evidence: `0x00599B23`, exact-match skips through `0x00599D95`, Cell payload writes)
- `[RESOLVED] OQ-21 — What precedes the first object on missing/changed preview? → Real Cells/dummy, ID-bearing types, Houses/Supers, then theater K; retail K=0.` (evidence: `0x00565C10`, `0x006686C0`, `0x004F54A0`, `0x00546538`)
- `[RESOLVED] OQ-22 — Do preview queues persist across Cancel/re-entry? → Yes; next Generate frees spread/growth and later rebuilds growth/spread.` (evidence: `0x00599A13/18`, `0x0059939B/A0`, dialog teardown `0x00595BC0`)

No material native-behavior blocker remains in this bounded prefix. The exact
absolute value for an arbitrary map/session is intentionally an input-derived trace
result, not an unresolved constant.

### Exhaustion, adversarial, and zero-add pass

1. **Could the final `+10,000` hide the need to count prior constructors?** No. It
   uses the saved variable value; every prior event shifts the entire later range.
2. **Could the first House pass be ignored because its objects are destroyed?** No.
   destruction does not decrement Scenario+0x214; all House/Super IDs remain spent.
3. **Could final registry lengths recover type cost?** No. Repeated names cost zero,
   lazy names interleave by reader order, and reset constructs a second generation.
4. **Could matching preview avoid duplicates because old Anims are eventually
   deleted?** No. Cells, Types, Houses, and Supers remain live too, and the first
   retained Cell/new-object overlap can occur immediately.
5. **Could the final Overlay drain safely start from an Overlay-only queue?** No.
   The native queue is shared. Fresh prefix emptiness is a proved precondition, not
   a replacement data type.

Cold checks of the Cell constructor/Resize/dummy sites, all shared-queue xrefs and
fresh House destruction, and the snapshot-to-reservation assembly overturned the
two stale assumptions: unconditional preview `1,000,001`, and an Overlay-private
drain. A final zero-add pass found no additional pre-snapshot Assign caller, prefix
queue writer, matching-preview helper constructor, active retail theater
allocation, or Tube reject-and-continue branch.

## 9. Visual/UI Composition Ledger

Not applicable. The bounded target is loader/identity/queue state, not preview
pixel composition; `RandMap.img`, shell chrome, and rendering were expressly
outside the claimed scope.

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Type constructor cost is a source-ordered first-new-name trace, not final registry lengths; retail explicit subtotal is 1,699 from 1,704 rows | `0x00668BF0`, `0x00679A10`, `rulesmd.ini` | trace is discarded; ParticleType projection can be mistaken for ID-bearing | `src/rules/ini_parser.rs`, `src/rules/ruleset.rs` | emit each actual ID-bearing Type construction once in native family/pass/lazy order | Fixtures A/B plus an existing-name/no-cost pass | do not seed from registry lengths or count ParticleType |
| Campaign and noncampaign Full_Init have different exact prefix skeletons; authored and `.SED` noncampaign both use two House/Resize passes | `0x00687F10`, `0x00689E90`, `0x005009B0`, `0x00565C10` | no consumed-once native-ID prefix | `src/sim/scenario_bootstrap.rs` and load plan | fold the exact formulas in section 3.5 with wrapping preincrement events | Fixtures A/B | do not hard-code 1,010,000 or omit disposable generations |
| reservation installs `wrap32(C_saved+0x2710)` after a shadowable theater window | `0x004AD026..0x004AD05F`, `0x00546538` | reservation/shadow trace absent | scenario/map load identity owner | snapshot once, retain shadowed assigns, set cursor from saved value | Fixtures E/H | do not add 10,000 to the post-theater current cursor |
| every allocated Tube row spends before parse; no reject-and-continue arm | `0x007283C0`, `0x00727FD0` | transaction can otherwise recount/admit validated rows | `src/map/tubes.rs` plus prefix transaction | bind one consumed-once Tube ID per constructed source row | Fixture F | do not silently skip a malformed row with zero ID |
| first Overlay is `C_saved ⊞ 10000 ⊞ T ⊞ 1`; children consume synchronously before next Overlay | `0x005FC380`, Mark path | Overlay/Anim native IDs are not tied to full prefix | `src/map/overlay.rs`, `src/sim/anim_class.rs` | continue the shared native cursor through Overlay and every actual child event | Fixture A | do not allocate child IDs post hoc or use stable handle as native ID |
| fresh common-drain entry is `[]`, but the queue is shared | queue xrefs, `0x005FD2E0`, `0x00725C70` | design can assume an Overlay-only queue | shared world lifecycle/deferred queue | assert empty fresh prefix contribution, then enqueue/drain reader events in native order | Fixture G | do not create a separate Overlay-only drain abstraction |
| exact-match preview emits no setup Assign and retains cross-class IDs; missing/changed emits `R+P_preview+HB+K_preview` | `0x00599650`, `0x00565C10`, type/House constructors | native preview ID/lifetime owner missing | shell preview lifecycle plus collision-free substrate | preserve independent runtime handles and globally duplicate numeric IDs; branch on the exact four-field key | Fixtures C/D | do not collide/reject by native ID or claim all previews start objects at 1,000,001 |
| preview queues free spread/growth before reset/branch, rebuild growth/spread, and persist across Cancel/re-entry | `0x00599A13/18`, `0x0059939B/A0`, `0x00595BC0` | queue lifetime absent | `PreviewNativeLifecycle` or exact equivalent | retain final queues; replace only at next Generate/launch in native order | Fixture C and queue persistence test | do not free on Cancel or invert free/rebuild order |

### Stale Docs / Follow-up Docs

- The prior claim in
  `docs/research/bridges/00-system-models/RMG_PREVIEW_ANIM_BUILDING_IDENTITY_LIFECYCLE_REINVESTIGATION_GHIDRA_REPORT.md`
  that every preview's first object receives `1,000,001` is replaced with:
  **“Only exact-match preview has no setup Assign; missing/changed preview consumes
  `R(W,H)+|P_preview|+HB(H_preview,S_preview)+K_preview` before the first generator
  object.”** That report is corrected in the same transaction.
- Any design wording that limits preview duplicate numeric IDs to old Anims is
  replaced with: **“Every retained Type/House/Super/real-or-dummy Cell/Anim/other
  untouched Abstract keeps its numeric ID across the exact-match counter reset;
  collision-free runtime handles are independent for every live class.”**
- Any design wording that initializes the reader drain as Overlay-only is replaced
  with: **“The live drain is shared; its fresh pre-reader prefix contribution is
  proved empty.”**

### Exact acceptance fixtures

### Fixture A — campaign prefix, Tubes, and ordinary child order

Controlled input:

- `E_campaign=0`;
- `P` emits five type events in order: HouseType, Side, two SuperWeaponTypes,
  BuildingType (`S1=2`);
- one campaign House;
- `W=2,H=3`, so `R=3*(2*2-1)+1=10`;
- two allocated well-formed Tube rows;
- first ordinary Overlay creates CellAnim then terrain Anim.

Expected:

```text
C_saved = 1_000_000 + 5 + 3 + 10 = 1_000_018
reserved cursor = 1_010_018
Tube IDs = 1_010_019, 1_010_020
Overlay ID = 1_010_021
CellAnim ID = 1_010_022
terrain Anim ID = 1_010_023
```

### Fixture B — noncampaign two-pass prefix

Controlled input:

- `E_multi=0`;
- two Houses and two SuperWeaponTypes in each pass (`HB=6` each);
- `R1=R2=10`;
- `P=5`;
- no Tubes.

Expected:

```text
C_saved = 1_000_000 + 6 + 10 + 5 + 6 + 10 = 1_000_037
reserved cursor = 1_010_037
first Overlay = 1_010_038
```

The event log must show pass-1 House/Supers and Cells before type reset, then the
new type events, pass-2 House/Supers, and in-place Cell re-ctors/dummy.

### Fixture C — preview exact match and guaranteed Cell-number reuse

Start from a prior successful missing-storage preview whose first real Cell owns
`1,000,001`. Generate again with the exact same four-field key and at least one
generator Building/Anim.

Expected:

- free old spread then growth queues;
- reset cursor to `1,000,000`;
- no Cell/Type/House/theater Assign event;
- retained Cell still owns `1,000,001`;
- first new object also receives `1,000,001` with no collision rejection;
- later rebuild growth then spread queues.

### Fixture D — preview changed/missing storage

Use `W=2,H=3`, the same five type events, one House, two Supers, retail theater.

Expected:

```text
R=10, P_preview=5, HB=3, K_preview=0
C_before_generator = 1_000_018
first generator object = 1_000_019
```

### Fixture E — wrap semantics

Given `C_saved=0xFFFFFFF0`, assert the installed post-reservation cursor is
`0x00002700`. Do not saturate, widen without truncating, or add to a post-snapshot
current value.

### Fixture F — Tube malformed/OOM policy

- One allocated malformed row records one Tube Assign and then returns a hard load
  error before Overlay; it does not silently continue with zero cost.
- Forced Tube allocation failure records no Tube Assign and returns a hard load
  error; it cannot produce an Overlay oracle.

### Fixture G — shared queue prestate

- Fresh no-identity pack: reader entry `[]`, common drain input `[]`.
- Fresh two-row ordinary pack: reader entry `[]`; if both rows queue once, common
  drain input is exactly `[overlay_handle_0, overlay_handle_1]` in decoded order.
- No House/Super/Type/Cell/Tube handle may appear in either queue.

### Fixture H — retail theater shadow count

Parse all six active YR theater INIs, collect every active `Tile%02dAnim`, and
assert 176 rows, 20 distinct names, and zero names absent from base
`rulesmd.ini [Animations]`. The trace still supports a custom absent name followed
by the set-from-snapshot overwrite.

## 11. Ghidra Annotation Candidates

No Ghidra metadata was changed.

| Address/source | Current metadata | Proposed metadata | Kind | Live proof | Status |
|---|---|---|---|---|---|
| `0x004AD026..0x004AD05F` | map-reader instruction sequence | comment: snapshot cursor; optional shadowed assigns; set from saved value plus `0x2710`, not current-cursor add | comment | `MOV ESI,[+0x214]`; `ADD ESI,0x2710`; later `MOV [+0x214],ESI` | worker-report-only |
| `0x005663D6`, `0x005663FC`, `0x005670F2` | Resize constructor calls | comment new Cell, in-place Cell re-constructor, and dummy as separate AssignUniqueID consumers | comment | all call `CellClass::Constructor @ 0x0047BBF0`, whose Assign is `0x0047BD8F/90` | worker-report-only |
| `0x00689E90` corridor | `ScenarioClass__Read_INI_Basic` | comment noncampaign second/final Create_Houses pass after rules reset | comment | Full_Init noncampaign branch and call order | worker-report-only |
| `0x007283C0` | `MapClass::ReadTubesINI` | comment Tube construction/Assign occurs before parse; OOM path faults | comment | constructor call to `0x00727FD0` precedes tokenization; null continuation dereferences | worker-report-only |
| `0x005FD692` common tail | Overlay-pack reader tail | comment shared deferred drain; fresh prefix entry state empty | comment | complete queue-writer xrefs plus unconditional drain call | worker-report-only |
| `0x00599A13/18`, `0x0059939B/A0` | tiberium queue calls | comment preview replacement order: free spread/growth before reset; rebuild growth/spread after generation | comment | direct calls and absence from dialog teardown | worker-report-only |
| exact-match branch in `0x00599650` | storage-key conditional | comment retains Type/House/Super/Cell/dummy/Anim numeric IDs across counter reset; native IDs globally non-unique | comment | skipped cleanup/Resize/helpers plus Cell payload writes excluding `+0x10` | worker-report-only |

## Sources

### Active `gamemd.exe`

- Scenario: `Set_Defaults @ 0x00683610`, `Clear_Scene @ 0x006851F0`,
  `Full_Init @ 0x00686B20`, `Create_Houses @ 0x00687F10`,
  `Read_INI_Basic @ 0x00689E90`, `NextUniqueID @ 0x0068BCB0`
- identity: `AbstractClass::Constructor_Full @ 0x00410170`,
  `AssignUniqueID @ 0x00410230`
- rules: `ResetTypeRegistriesAndReloadRules @ 0x006686C0`,
  `RulesClass::Process @ 0x00668BF0`, `ReadTypeData @ 0x00679A10`
- Houses: `HouseClass::Constructor @ 0x004F54A0`, destructor `0x004F7140`,
  `SuperClass::Constructor @ 0x006CAF90`, campaign House reader `0x005009B0`
- Cells/map: `CellClass::Constructor @ 0x0047BBF0`,
  `MapClass::Resize @ 0x00565C10`, map reader `0x004ACE70`,
  snapshot/reservation `0x004AD026..0x004AD05F`
- theater: `Read_Theater_TileSets_INI @ 0x00545150`,
  `AnimTypeClass::FindOrAllocate @ 0x00428B80`, call `0x00546538`
- post-reservation: TagType helper `0x006E6310`, `ReadTubesINI @ 0x007283C0`,
  `TubeClass::Constructor @ 0x00727FD0`, helpers `0x00465CC0/0x004F42F0`,
  `ReadMapOverlayPacks @ 0x005FD2E0`, `OverlayClass::Constructor @ 0x005FC380`
- shared lifecycle: `FUN_00534450`, queue globals `0x00B0F69C/0x00B0F6A8`,
  `DrainDeferredFinalizationQueue @ 0x00725C70`
- preview: `RandomMapGenerator__Generate @ 0x00598960`,
  `InitMapFromSyntheticINI @ 0x00599650`, `FUN_00689880`
- tiberium queues: FreeSpread all `0x00722390`, FreeGrowth all `0x00722E50`,
  InitGrowth all `0x00722D00`, InitSpread all `0x00722240`

### Retail data

- `ini/rulesmd.ini`: active type registries and `[Animations]`
- `ini/temperatmd.ini`, `ini/snowmd.ini`, `ini/urbanmd.ini`,
  `ini/urbannmd.ini`, `ini/desertmd.ini`, `ini/lunarmd.ini`:
  active `Tile%02dAnim` rows

### Rust inspected

- `src/rules/ini_parser.rs`
- `src/rules/ruleset.rs`
- `src/sim/scenario_bootstrap.rs`
- `src/sim/world/substrate.rs`
- `src/sim/anim_class.rs`
- `src/map/tubes.rs`
- `src/map/overlay.rs`
- `docs/plans/2026-08-28-active-retail-bridge-parity-design.md`

### OpenTS navigation leads only

- `C:\Users\enok\Documents\OpenTS\code\display.cpp`
- `C:\Users\enok\Documents\OpenTS\code\scenario.cpp`
- `C:\Users\enok\Documents\OpenTS\code\map.cpp`

### Corroborating current active-binary reports

- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAY_EPHEMERAL_OBJECT_FINALIZATION_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/TERRAIN_ATTACHED_ANIM_LOAD_LIFECYCLE_SIDE_EFFECTS_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_TIBERIUM_GERMINATE_SIDE_EFFECT_REINVESTIGATION_GHIDRA_REPORT.md`
