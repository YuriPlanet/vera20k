# DiskLaser.Radius

A `DiskLaser=yes` weapon draws the Vortex ring from a fixed 16-point table
built for a 240-lepton radius. Phobos lets a weapon choose the radius; the
ring is rebuilt from the hook's sector constants when the value differs.

```ini
[DISKLASER]                        ; Weapon section
DiskLaser=yes
DiskLaser.Radius=240               ; double, default DiskLaserClass::Radius
```

Reader: `Ext/WeaponType/Body.cpp:86`, `Valueable<double>` constructed at
`DiskLaserClass::Radius` (240) in `Ext/WeaponType/Body.h:28,123`.

## Native body

- `DiskLaser_Circle` (hook `0x4A757B`,
  `Ext/WeaponType/Hooks.DiskLaserRadius.cpp:27-42`) runs inside the ring draw
  with the weapon. When the weapon's `DiskLaser.Radius` differs from the last
  applied radius, it rebuilds the shared 16-entry `DrawCoords` table as
  `(int)(cos * radius)`, `(int)(sin * radius)` from its literal sector
  constants. `OldRadius` starts at 240, so the original literal table stays
  until a weapon with another radius draws.

## VERA

`WeaponType::phobos_disk_laser_radius` reads the key. `tick_disk_lasers`
resolves the shot's weapon and derives its ring with
`phobos_disk_laser_ring`; the literal 240 ring stays byte-identical to the
recorded `0x8A0180` table and any other radius is rebuilt with the hook's
constants and C-style truncation.

## Tests

- `src/rules/disk_laser_tests.rs` —
  `disk_laser_radius_reads_the_weapon_key_over_the_constructor_240` (an
  unset key is 240, `DiskLaser.Radius=120` is 120).
- `src/sim/world/disk_laser.rs` —
  `the_phobos_radius_rebuilds_the_ring_and_240_keeps_the_literal_table`
  (240 returns the literal table; 120 returns the truncated cos/sin ring).

The rebuild was mutated red and restored.

## Residuals

- Native rebuilds one global table, so a later 240-radius shot after a custom
  one draws the recomputed 240 points (one point is one pixel off the literal
  table, `[-53,-233]` instead of `[-54,-233]`). VERA always uses the literal
  table at 240; the difference is a single pixel on one ring point.
