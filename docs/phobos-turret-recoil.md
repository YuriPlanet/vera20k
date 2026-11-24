# TurretRecoil.Suppress

A unit's fired shot arms its voxel turret/barrel recoil. Phobos lets a
weapon suppress that record per shot.

```ini
[GUN]                      ; [WeaponTypes] section
TurretRecoil.Suppress=no   ; Valueable<bool>, default no
```

Reader: `Ext/WeaponType/Body.cpp:138` (constructor `Ext/WeaponType/Body.h:163`,
`false`).

## Native body

`TechnoClass_FireAt_TurretRecoil` (`0x6FF0DD`,
`Ext/Techno/Hooks.Firing.cpp:821-836`) always returns `SkipGameCode`, so the
vanilla recoil arm never runs on a Phobos build. When the fired weapon
resolves `TurretRecoil.Suppress` false the hook calls
`UnitExt::RecordRecoilData` directly; when true it returns without that call,
so the unit's recoil state is never armed for that shot.

## VERA

`PhobosWeaponExtras::turret_recoil_suppress` reads the key.
`combat::world_fire`'s post-launch recoil arm skips
`GameEntity::fire_voxel_recoil` when it resolves true, leaving the entity's
`VoxelRecoil` state clear; the shot itself, rearm and every other FireAt tail
are unchanged.

## Tests

- `src/sim/combat/phobos_turret_recoil_tests.rs` — a turreted unit with ART
  `TurretRecoil=yes` arms recoil after a stock shot and stays clear with
  `TurretRecoil.Suppress=yes` while the bullet still launches. The gate was
  mutated red (recoil armed under the key) and restored.

## Residuals

- The Ares `RecordRecoilData` history (voxel-oracle recoil corpus) is a
  drawing input; only the retained state and its clear-on-suppress are
  modelled, not the drawn displacement beyond `voxel_recoil()`.
