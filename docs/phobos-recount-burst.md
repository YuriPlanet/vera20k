# Recount burst index

`[General] RecountBurst=yes` and its per-type `[TechnoType] RecountBurst=`
(`Nullable<bool>`) make a retained burst index recount against the previous
weapon when the weapon changes, or when the shot sits idle long enough.

```ini
[General]
RecountBurst=yes

[TANK]
RecountBurst=yes   ; overrides the global
```

Read by `Ext/Rules/Body.cpp:382` (default `no`, constructor
`Ext/Rules/Body.h:803`) and `Ext/TechnoType/Body.cpp:1031` (default unset).

## Native evidence

- `TechnoExt::OnEarlyUpdate` (`Ext/Techno/Body.Update.cpp:22`) calls
  `UpdateRecountBurst` (`:544-566`). With no target, a non-zero
  `CurrentBurstIndex`, `RecountBurst` on and
  `LastFireBulletFrame + max(ROF, 30) <= CurrentFrame`, it computes
  `int(CurrentBurstIndex / Burst * ROF * AE.ROFMultiplier) - max(ROF, 30)`; a
  positive value arms `ChargeTurretDelay` and `RearmTimer`, then
  `CurrentBurstIndex` is zeroed.
- `TechnoClass_FireAt_UpdateWeaponType` (`0x6FDD7D`,
  `Ext/Techno/Hooks.Firing.cpp:609-633`) does the same on a weapon change
  against `LastWeaponType`'s own `Burst`/`ROF`, using
  `CurrentFrame - LastFireBulletFrame`; a positive ROF re-arms, zeros the
  index, stores the new weapon and refuses the shot. Every `FireAt` stores
  `LastWeaponType`.

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::recount_burst` (`[General]`).
- `rules::object_type::ObjectType::phobos_recount_burst` (`Nullable`).
- `sim::game_entity::GameEntity::phobos_last_weapon_type` (`LastWeaponType`
  as the interned weapon section).
- `sim::combat::burst::update_phobos_recount_burst`, called from
  `techno_common_post` before `UpdateRearmInEMPState`.
- `sim::combat::world_fire::emit_admitted_fire`'s `0x6FDD7D` arm, after the
  LimboLaunch gate and before the InLimbo refusal.

`WeaponBurst::reset` is the `CurrentBurstIndex = 0` store, and
`GameEntity::rearm_after_fire` writes both `ChargeTurretDelay` and the rearm
timer.

## Residuals

- `LastWeaponType` is the interned weapon section name; stock and layered
  reads give one instance per section, so identity comparison matches the
  native pointer comparison.
- `UpdateRecountBurst` runs once per visit in the shared late Techno slot; the
  native step order places it just before `UpdateRearmInEMPState`, which is
  preserved, but `UpdateLastTargetCrd`/`CheckDeathConditions` already ran in
  this visit. Only a fatal visit can observe the difference.
