# Animation-created units (`CreateUnit.*`)

An animation type can create one Techno when the anim reaches its
actual-effects frame, on the anim's cell and for a chosen house. Phobos reads
`CreateUnit` as a `Nullable<TechnoTypeClass*>` (`Ext/AnimType/Body.cpp:130-149`);
when it is set, the same ART section is read through
`CreateUnitTypeClass::LoadFromINI`
(`New/Type/Affiliated/CreateUnitTypeClass.cpp:3-22`, declarations and
constructor defaults `CreateUnitTypeClass.h:5-30`). Fifteen keys; `Owner` and
`RemapAnim` already install the anim's own house.

```ini
[ANIM]                                   ; Animation ART section
CreateUnit=                              ; Nullable<TechnoTypeClass*>, none clears
CreateUnit.Facing=0                      ; Valueable<DirType>
CreateUnit.InheritFacings=no             ; Valueable<bool>
CreateUnit.InheritTurretFacings=no       ; Valueable<bool>
CreateUnit.RandomFacing=yes              ; Valueable<bool>, constructor default yes
CreateUnit.RemapAnim=no                  ; Valueable<bool>
CreateUnit.Mission=Guard                 ; Valueable<Mission>
CreateUnit.AIMission=                    ; Nullable<Mission>
CreateUnit.Owner=Victim                  ; Valueable<OwnerHouseKind>
CreateUnit.RequireOwner=no               ; Valueable<bool>
CreateUnit.AlwaysSpawnOnGround=no        ; Valueable<bool>
CreateUnit.SpawnParachutedInAir=no       ; Valueable<bool>
CreateUnit.ConsiderPathfinding=no        ; Valueable<bool>
CreateUnit.SpawnAnim=                    ; ValueableVector<AnimTypeClass*>
CreateUnit.SpawnHeight=-1                ; Valueable<int>
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x4226F0` | `AnimClass_CTOR_CreateUnit_MarkOccupationBits` | After the anim's constructor, a type with `CreateUnit` calls `MarkAllOccupationBits(GetCell()->GetCoordsWithBridge())`, the three infantry sub-cell bits `0x04|0x08|0x10` (`0x71C146`). `Ext/Anim/Hooks.AnimCreateUnit.cpp:36-50`. |
| `0x424932` | `AnimClass_AI_CreateUnit_ActualEffects` | At the actual-effects point: `UnmarkAllOccupationBits` on the same coord, then the facing (one `RandomRanged(0, 255)` when `RandomFacing`, else `Facing`; `InheritFacings` + `FromDeathUnit` uses the stored death facing; `InheritTurretFacings` the stored turret facing for a UnitType with a turret), then `TechnoTypeExt::CreateUnit`; a `MakeInfantry=` type continues to its own arm. `:52-80`. |
| `0x737F6D`, `0x738807` | `UnitClass_TakeDamage_Destroy`, `UnitClass_Destroy_DestroyAnim` | `AnimTypeExt::ProcessDestroyAnims` (`Ext/AnimType/Body.cpp:8-73`) creates the unit's `DestroyAnim=` with the destroyed unit as `SetInvoker`, `FromDeathUnit`, the body `PrimaryFacing.Current().GetDir()` and, for a turreted type, `SecondaryFacing.Current()`. |

`TechnoTypeExt::CreateUnit` (`Ext/TechnoType/Body.cpp:587-733`) refuses
`AbstractType::BuildingType`; a missing or defeated passed House falls to
`HouseClass::FindCivilianSide()` unless `RequireOwner`; `ConsiderPathfinding`
searches `MapClass::NearByLocation` when the cell is not clear; the Z is the
cell floor (`GetCellFloorHeight` plus the bridge deck) plus `SpawnHeight`, or
`max(floor, base)` / the floor with `AlwaysSpawnOnGround`; 2+ `CellHeight` in
the air with `SpawnParachutedInAir` uses `SpawnParachuted`; otherwise
`Unlimbo` at the facing. On success it sets the secondary facing, calls
`CreateRandomAnim(SpawnAnim, location, pInvoker, pInvokerHouse, true)`, queues
`UnitMission` (`AIUnitMission` when set and the owner is not human-controlled)
and marks the owner's `RecheckTechTree` unless MultiplayPassive.

`AnimExt::SetAnimOwnerHouseKind` (`Ext/Anim/Body.cpp:108-145`) gives a
`CreateUnit` anim type its own `Owner` kind with the invoker House as the
default (random allowed). VERA resolves that kind where the anim is created:
the combat-detonation anim owner and the Unit destruction anims
(`sim::combat::destruction_effects`) apply `CreateUnit.Owner`; other shared
spawn callers pass their own house.

## Where VERA20K owns it

- `rules::phobos_create_unit::CreateUnitSettings` holds all keys with the
  constructor defaults; `rules::art_data` reads it from the ART section and
  keeps the `none` clearing rule.
- `sim::anim_class` marks the cell in the anim constructor
  (`spawn_anim_at_world_with_constructor`) and runs the actual-effects arm at
  the same `AnimBoundary::Complete` point as `MakeInfantry`, once per anim
  (`AnimObject::create_unit_fired`).
- `sim::anim_create_unit` is the arm: unmark, facing selection, owner and
  `require_owner`/defeat handling, `BuildingType` refusal, the placement
  (`SpawnHeight` / `AlwaysSpawnOnGround` / bridge deck), `OnBridge`,
  `ConsiderPathfinding`, the parachute or ordinary `Unlimbo` through the
  shared construction/reveal owners, the spawn anim, the mission queue and
  the tech-tree recheck.
- Death facts live on the anim (`from_death_unit`, `death_unit_facing`,
  `death_unit_has_turret`, `death_unit_turret_facing`) and on the anim's
  invoker (`invoker`, `invoker_house`); `sim::combat::destruction_effects`
  records them from the destroyed Unit's body/turret facing and the Unit's
  House.
- `CreateUnit.SpawnAnim` uses the shared `spawn_random_owned_anim`
  (`CreateRandomAnim`'s random pick and construction owner). The invoker flag
  stores the anim's invoker; `ownedObject` stays false, so the SpawnAnim is
  not owner-attached. The house is the invoker house, else the invoker's
  owner.

## Phobos quirks kept

- `RandomFacing` constructs **yes** and overrides `CreateUnit.Facing` unless
  the key is `no`; the draw is one `RandomRanged(0, 255)`.
- `CreateUnit` is `Nullable`: absent keeps the previous state, `none` clears
  it. A name no `TechnoType` resolves is kept as present here (native nulls
  the nullable at load); the spawn then refuses for lack of a type.
- The anim's occupation bits are chosen by `GetCoordsWithBridge()`: the
  ground plane, or the deck on a cell carrying the live structural bridge
  flag (`+0x140 & 0x100`). The anim's height does not choose the plane.
- The building refusal happens before any object is created, but after the
  facing draw.
- A missing or defeated anim house falls to the first `Civilian`-side House
  in HouseClass array order unless `RequireOwner`.
- `SpawnHeight` applies only when `>= 0` and `AlwaysSpawnOnGround` is `no`;
  `AlwaysSpawnOnGround` pins the cell floor and also overrides
  `SpawnParachutedInAir`.
- The parachute arm needs a non-Aircraft type, 2+ `CellHeight` of height and
  no `AlwaysSpawnOnGround`.
- `AIMission` replaces `Mission` only when it is set and the owner is not
  human-controlled.
- `SpawnAnim` names resolve at use through `RuleSet::has_anim_type`; an empty
  or fully unresolved list draws nothing (native's load-time vector would be
  empty too).
- `InheritTurretFacings` needs `FromDeathUnit` and a stored turret fact; the
  turret facing is the destroyed unit's `SecondaryFacing.Current()` rounded
  to the 8-way DirType.

## RNG draws

- `RandomFacing` (default yes): one Scenario `RandomRanged(0, 255)`.
- `CreateUnit.Owner=Random`: one Scenario `RandomRanged(0, houses - 1)` at
  the anim owner install (combat-detonation and Unit-destruction paths).
- `CreateUnit.SpawnAnim`: one Scenario `RandomRanged(0, size - 1)` over the
  names that resolve, when the list is non-empty; none for an empty list.
- `ConsiderPathfinding`'s nearby search is the frame-counter selection and
  draws nothing.
- The shared unit constructor consumes its own Scenario draw on this path;
  the regression test compares against a `RandomFacing=no` control, so it
  asserts only the one extra draw the anim arm adds.

## Residuals (trigger · impact · frequency · risk)

- **Fly/Jumpjet landing orders.** The `FlyLocomotion`/`JumpjetLocomotion`
  blocks are not modeled; a spawned air unit keeps its placement pose. The
  `IsFallingDown`/`OnParachuted` half of that branch is covered by the
  `OnParachuted` latch (`docs/phobos-unlimbo-detonate.md`), but the two in-air
  branches that raise the fall (`Ext/Foot/Body.cpp:784`,
  `Ext/TechnoType/Body.cpp:705`) are those Phobos locomotion blocks and stay
  unported. Trigger: a CreateUnit type whose spawned type is an Aircraft or
  Jumpjet. Frequency: authored mods. Risk: movement behavior; no fall damage
  is taken, since no fall starts.
- **`allowBridges`.** Native tests `GroundType[Clear].Cost[SpeedType] > 0`;
  VERA treats every non-Winged speed type (Aircraft uses Wheel, as native
  does) as bridge-capable. Trigger: a type whose Clear cost is zero.
  Frequency: modded. Risk: placement on a bridge.
- **`ConsiderPathfinding` search.** VERA reuses
  `Simulation::techno_nearby_location` (`TechnoClass::NearbyLocation`), which
  restricts the search to the seed cell's zone and reads the constructed
  unit's own SpeedType, where native calls `MapClass::NearByLocation` with
  zone `-1` and its own Wheel/Normal aircraft substitution; the pre-check
  also uses `CheckCellPassability` with level `None` rather than
  `CellClass::IsClearToMove`'s exact arguments. Trigger: a blocked spawn cell
  near a zone boundary. Frequency: authored mods. Risk: the unit stays on the
  blocked cell. No regression test (needs a mixed passability map).
- **`ScenarioInit` bracket.** Native raises `Unsorted::ScenarioInit` around
  `Unlimbo` when the cell has no building or pathfinding is off; VERA's
  reveal owner does not model that bracket. Trigger: every ordinary
  CreateUnit spawn. Frequency: always. Risk: only effects that read
  ScenarioInit during Unlimbo.
- **`CreateUnit.Owner=Invoker`/`Killer` on destroy anims.** `ProcessDestroyAnims`
  passes the killer House (`TakeDamage`) as native's invoker; VERA's
  `unit_death_explosion` does not receive a killer, so those selectors fall
  back to the victim House. `Victim`, `Civilian`, `Special`, `Neutral` and
  `Random` resolve. Trigger: a destroy anim with an invoker-owner kind.
  Frequency: authored mods. Risk: wrong anim colour/remap and SpawnAnim
  house.
- **Generic anim spawns.** `spawn_owned_anim` callers that pass a resolved
  house (passenger deletion, `PlayAnimAt`, shield break) do not re-select a
  `CreateUnit.Owner` kind for the anim itself, unlike native's
  `SetAnimOwnerHouseKind` hook; the anim still spawns its unit for that
  house. Trigger: those spawn paths with `CreateUnit.Owner` set. Frequency:
  authored mods. Risk: anim colour only.
- **`SpawnAnim` house fallback.** Native leaves the anim unowned when
  neither the invoker house nor the invoker's owner exists; the shared VERA
  spawn owner requires a house and falls back to the spawned unit's owner.
  Trigger: a CreateUnit anim without invoker facts. Frequency: rare. Risk:
  anim colour only.
- **Anim invoker house.** Native `ProcessDestroyAnims` calls
  `SetInvoker(pThis)` with no house; VERA stores the victim House beside the
  invoker. Consumers that need the owner read it from the entity. Trigger:
  destroy-anim consumers. Frequency: authored mods. Risk: none observed.
- **Unresolvable `CreateUnit=`.** Native nulls the reference at load, so the
  type marks no sub-cell bits and draws no facing. VERA keeps the authored
  name and applies the same null result at use (constructor mark gate and
  actual-effects early-out). Same observable state; one fewer load-time
  validation. Trigger: a `CreateUnit=` naming no loaded unit type. Frequency:
  invalid mod data. Risk: none.
- **`Next=` chain.** The one-shot CreateUnit latch resets when `Next=` switches
  the instance's type, so a chain of two `CreateUnit=` types spawns each; the
  reset matches the per-type arm, which native re-evaluates after the switch.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
