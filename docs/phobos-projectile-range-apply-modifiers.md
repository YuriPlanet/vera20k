# ProjectileRange.ApplyModifiers

A `Ranged=` projectile carries the weapon's `ProjectileRange` as its travel
budget. Phobos can fold the firer's range modifiers into that budget:
`GetRangeWithModifiers` (the veteran `Range` ability and AttachEffect range
effects).

```ini
[CombatDamage]
ProjectileRange.ApplyModifiers=yes  ; boolean, default yes

[COIL]                              ; Weapon section
ProjectileRange.ApplyModifiers=no   ; nullable, unset -> global
```

Readers: the global at `Ext/Rules/Body.cpp:650` (`Valueable<bool>`,
constructor yes at `Ext/Rules/Body.h:544,1044`); the weapon at
`Ext/WeaponType/Body.cpp:88` (`Nullable<bool>` unset, `Body.h:30,125`).

## Native body

- `BulletClass_AI_Ranged` (`0x467B8E`, `Ext/Bullet/Hooks.cpp:636-659`) starts
  every step from `maxRange = pThis->Range`; only a positive range with a
  weapon and owner is replaced by `WeaponTypeExt::GetRangeWithModifiers(weapon,
  owner, maxRange)` when the weapon's nullable key else `[CombatDamage]`'s
  global is set. `DistanceTraveled >= maxRange` then detonates the bullet.
- `GetRangeWithModifiers` has two overloads. The two-argument one
  (`Ext/WeaponType/Body.cpp:374-396`) supplies the weapon's own range and
  returns `-512` before anything else. The three-argument one
  (`Body.cpp:398-443`) takes an explicit range, has no sentinel arm, swaps an
  open-topped passenger for its transport first, multiplies by
  `max(VeteranRange.Get([General] VeteranRange), 0.0)` when the swapped firer
  has `AdditionalAbility(Range)`, then folds every active AttachEffect whose
  weapon lists admit the weapon, with a final floor at zero.
- `TechnoClass_InRange_WeaponRange` (`0x6F7248`,
  `Ext/Techno/Hooks.WeaponRange.cpp:134-186`) uses a live `GetTechnoKeepRange`
  maximum instead of `GetRangeWithModifiers`; the folds never scale it.

## VERA

`CombatDamageDefaults::phobos_projectile_range_apply_modifiers` and
`WeaponType::phobos_projectile_range_apply_modifiers` read the two keys.
`attach_effect` owns both native overloads:
`weapon_range_with_modifiers` (two-argument: weapon range plus the `-512`
sentinel) and `get_range_with_modifiers` (three-argument: transport source
first, veteran arm, AttachEffect fold). InRange, weapon-range queries, the
force-weapon sweep, area fire, the trajectory distances and the
target-distance helper call the overload their native site uses;
`compute_range_target` lets a live KeepRange bypass both.
`attach_effect::initial_projectile_range` applies the `maxRange > 0` guard,
the two keys and the fold to a `Ranged=` projectile at fire time (the
ordinary fire and the Ares airburst child), whose remaining range the Ares
bullet update already spends.

## Tests

- `src/sim/attach_effect.rs` —
  `the_veteran_range_ability_scales_get_range_with_modifiers` (rookie,
  veteran, elite, type-over-global, elite-only tier, the three-argument
  overload scaling an explicit `-512`, the two-argument sentinel) and
  `initial_projectile_range_reads_the_global_and_weapon_keys` (default fold,
  global off, weapon over global, the `maxRange > 0` guard, negative and
  sentinel ranges untouched).
- `src/sim/combat/in_range.rs` —
  `a_live_keep_range_bypasses_the_range_modifiers`.

The veteran arm, the key gate, the `range <= 0` guard and the KeepRange
bypass were each mutated red and restored.

## Residuals

- VERA models the stock `Ranged=` distance budget (the `BulletClass_AI_Ranged`
  `DistanceTraveled >= Range` test at `0x467B8E`) only in the Ares bullet
  update today, so this key's consumer is active in `ares,phobos` builds; a
  phobos-only build has no ranged budget to scale, though
  `initial_projectile_range` is tested directly.
- VERA applies the fold when the bullet is created rather than on every
  update; the firer's rank, effects and transport at creation are the values
  the native update reads on the first step, so the spent budget matches
  unless one of them changes mid-flight.
- `GetRangeWithModifiers`'s `CanOccupyFire` arm (an occupied building's
  `OccupyWeaponRange`) is not part of this owner; callers pass their own base
  range as native's two-argument call site does.
- Phobos airburst children (`sim::combat::phobos_airburst::spawn_child`) are
  created without an Ares `Ranged=` budget, so `BulletClass_AI_Ranged` never
  reaches them; native `SimulatedFiringUnlimbo` sets `pBullet->Range =
  ProjectileRange` (`Ext/Bullet/Body.cpp:322`). Pre-existing gap in the
  airburst chain, not this key's.
