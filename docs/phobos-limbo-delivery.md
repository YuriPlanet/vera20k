# Limbo-delivered and limbo-killed buildings

A superweapon can create buildings that exist for power, factories and
prerequisites without ever being built, and can later remove them by an
author-chosen id. Phobos reads the keys in `SWTypeExt::LoadFromINIFile`
(`src/Ext/SWType/Body.cpp:159-169`) and runs the two arms from
`SWTypeExt::FireSuperWeaponExt` (`Ext/SWType/FireSuperWeapon.cpp:13-40`),
before `SW.Next`.

```ini
[SW]                                ; SuperWeaponType
LimboDelivery.Types=                ; building list
LimboDelivery.IDs=                  ; int list, one LimboID per type
LimboDelivery.RollChances=          ; float list, one die per weighted roll
LimboDelivery.RandomWeights=        ; int list alias for slot 0
LimboDelivery.RandomWeights0..N=    ; int lists, until the first empty one
LimboKill.IDs=                      ; int list of LimboIDs to remove
LimboKill.Counts=                   ; int list of removal caps
LimboKill.AffectsHouse=owner        ; AffectedHouse mask, constructor Owner
LimboKill.Affected=                 ; deprecated spelling, read first
```

`LimboDelivery.Types` resolves each token through `BuildingTypeClass::Find`;
an unknown name is dropped at read time. The weights pair behaves like
`SW.Next`'s (numbered lists stop at the first empty key, the unnumbered alias
rewrites or creates slot 0). `LimboKill.Affected` is read before
`LimboKill.AffectsHouse`, so the canonical key wins when both are present.

## Behavior

At the launch tail, in native order:

1. **`ApplyLimboDelivery`** (`:172-201`). With weights, `WeightedRollsHandler`
   rolls once per `RollChances` entry and picks one type per roll; without
   weights every listed type is delivered in order. The `IDs` cursor starts
   at -1 and only advances when the type index has an entry, so a longer
   `Types` list reuses the last id.
   `LimboCreate` (`:87-170`) refuses when the type's `BuildLimit` (including
   its `UndeploysInto` type) is reached, creates the building for the owner at
   the constructor's cell, marks it alive and on the map, registers its gain,
   requests a tech-tree recheck, stamps `BuildingExt::LimboID`, appends it to
   `HouseExt::OwnedLimboDeliveredBuildings` and to the separate limbo tracking
   counter (unless `Insignificant`/`DontScore`).
2. **`ApplyLimboKill`** (`:203-262`). For every house the
   `CanTargetHouse(AffectsHouse)` test accepts, each unique `LimboKill.IDs`
   entry removes up to its `Counts` entry (all when the list is shorter)
   buildings whose `LimboID` matches. Each removed building then runs
   `RemoveFromLimboTracking` (same `Insignificant`/`DontScore` guard),
   `Stun()`, `Limbo()`, `RegisterDestruction(nullptr)`, `UnInit()`.

## Where VERA20K owns it

- `rules::phobos_limbo::PhobosSuperWeaponLimbo` reads the keys in the ordered
  Process superweapon body and is projected into
  `RuleSet::phobos_super_weapon_limbo`, hashed under
  `phobos-super-limbo-v1`.
- `sim/superweapon/limbo.rs` runs the two arms from
  `sim/superweapon/chaining.rs::apply_phobos_launch_tail`, in the native slot
  before `SW.Next`; the weighted selection reuses
  `chaining::weighted_rolls`.
- `HouseTracking` owns `HouseExt::OwnedLimboDeliveredBuildings`
  (`limbo_delivered: Vec<(stable_id, LimboID)>`) beside the existing separate
  limbo counters; it is saved and hashed. The kill reuses the lifecycle
  owners (`stun`, `techno_limbo_with_context`, `record_kill_and_uninit`) and
  `combat::can_target_house` for the mask.

## Draws and timers

One Scenario `RandomDouble` per weighted delivery roll. No timer writes of
its own; the kill's lifecycle calls write theirs (stun, limbo, destruction
bookkeeping).

## Native anchors

`SuperClass::Launch` (`0x6CC390`), with its tail at `0x6CDE40`, is where the delivery runs
(`Ext/SWType/Hooks.cpp:1..2`). `HouseClass_RecalcCenter_LimboDelivery` (`0x4FD1CD` and `0x4FD463`,
`Ext/House/Hooks.cpp:94..95`) is the second site that has to see the delivered buildings.

## Residuals

- `LimboCreate` places at the constructor's cell `(0, 0)`; native keeps a
  building whose `Place` fails, while VERA's Unlimbo admission refuses it and
  skips that delivery. Trigger: cell `(0, 0)` blocked or an occupied target.
- A captured delivered building stays in the old house's list (native's list
  is on the owning HouseExt and is not moved either); the kill then removes it
  from the old list and destroys it through its current owner's tracking.
- The `ConYards`/`SecretLabs`/`FactoryPlants`/power-plant-enhancer lists are
  derived from the normal building projection, not registered separately; a
  delivered building that the type projection excludes would be missing from
  them.
- Phobos also queues the building in `ScenarioExt::AutoDeathObjects` and may
  start its timer; VERA's existing AutoDeath owner evaluates every object
  each tick, so no second registration is kept.
- `ApplyDetonation` (the next tail arm) belongs to a separate chain and is not
  ported here.
