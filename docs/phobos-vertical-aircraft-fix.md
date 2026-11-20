# Vertical projectiles fired by aircraft

Phobos fixes the launch of `Vertical=` bullets fired by an aircraft: the bullet
keeps the velocity TechnoClass::FireAt gave it instead of the aircraft
`Fire_At` course that scales it to the aircraft's speed.

```ini
[Comment]                      ; BulletType section
Vertical=yes                   ; stock flag
Vertical.AircraftFix=          ; Nullable<bool>, [General] fallback

[General]
Vertical.AircraftFix=yes       ; Valueable<bool>, default yes
```

Readers: `Ext/BulletType/Body.cpp:36` (`Ext/BulletType/Body.h:36`, constructed
unset `:103`) and `Ext/Rules/Body.cpp:581` (`Body.h:492`, constructed true
`:990`).

## Behavior

`AircraftClass::Fire_At @ 0x00415EE0` normally rewrites the bullet's velocity
by its type's `ROT=`: ROT 0 to the aircraft's Apparent_Speed and
SecondaryFacing (`0x00415F39..0x004160B5`), ROT 1 toward the target at weapon
0's `Speed=` (`0x004160CF..0x00416318`). Phobos hooks the ROT dispatch head at
`0x415F25` (`Misc/Hooks.BugFixes.cpp:2423`): a bullet with `HasParachute` or
with `Vertical=yes` and an effective `Vertical.AircraftFix` jumps to
`0x41631F`, skipping both ROT arms, so it leaves at FireAt's own velocity and
falls under its own trajectory. The bullet type's `Nullable` key overrides the
`[General]` switch.

## Where VERA20K owns it

`rules::projectile_type` reads the bullet key, `rules::phobos_globals` the
`[General]` fallback. `sim::aircraft::fire_at::AircraftShot` carries the
effective flag beside `ROT`/velocity and skips the
[`AircraftBulletCourse`] computation; the production host
(`sim::combat::aircraft_release`) resolves it from the fired weapon's bullet
type.

## Residuals

- `BulletClass::HasParachute` is not modelled: VERA has no parachuted bullet
  flag (`Parachute=` BulletType key), so that arm of the hook is dormant. A
  retail type with a parachuted bullet and a ROT course would skip the course
  natively and here still apply it. No retail bullet authors it.
- The gate is applied only at the two ROT arms, matching the hook's jump over
  `0x415F25..0x41631F`; other ROT values already keep FireAt's velocity.
- `Apparent_Speed`'s SimFixed-vs-double residual in `fire_at.rs` is unchanged;
  a fixed Vertical bullet no longer reads it.
