# Optional Phobos projectile gravity

Build with `--features phobos` to give a projectile its own ballistic gravity
in place of the global one.

```ini
[SOMEPROJECTILE]              ; a [Projectiles] entry
Gravity=                      ; double, defaults to [General] Gravity= (retail 6)
```

The key is a Phobos `Nullable<double>`: an empty, `<default>` or invalid value
leaves it unset, and the projectile uses `[General] Gravity=`. Phobos's
`BulletTypeExt::GetAdjustedGravity` halves whichever value applies for a
`Floater=yes` projectile.

## Native sites

Retail loads the integer `[General] Gravity=` (`Rules+0x16B8`) wherever a
ballistic projectile needs it, and `0x0048ACF0` returns half of it for a
Floater. Phobos's `ApplyGravity` hooks load the adjusted value instead:

| Hook | Site | VERA20K |
| --- | --- | --- |
| `BulletClass_AI_ApplyGravity` | `0x004671B9`, the flight step | `sim/projectile.rs` |
| `TechnoClass_Targeting_ApplyGravity` | `0x006F7481`, InRange's arcing reach | `sim/combat/in_range.rs` |
| `TechnoClass_FireAt_ApplyGravity` | `0x006FECB2`, the launch | `sim/combat/world_fire.rs` |
| `WeaponTypeClass_SetSpeed_ApplyGravity` | `0x00772A0A`, the rules-time `Speed` | `rules/native_processing.rs` |
| `WeaponTypeClass_GetSpeed_ApplyGravity` | `0x00773087`, the fire-time speed | `sim/projectile/launch.rs` |

Shrapnel children carry their own projectile's value. The flight-step hook
replaces only the load at `0x004671B9`: a Floater shell still discards that
value at `0x004671C3` and calls `0x0048ACF0`, so in flight a Floater falls at
half the Rules gravity whatever its `Gravity=` says. The other hooks replace
the whole Floater selection and use half the type value.

Source: `src/Ext/BulletType/Body.cpp` and `src/Ext/Bullet/Hooks.cpp` (by
secsome), with the WeaponType hooks in `src/Ext/WeaponType/Hooks.cpp`.

## Residuals

- `TechnoClass_FireAngle_6FDA00_ApplyGravity` (`0x006FDAA6`): VERA20K does not
  port `TechnoClass` virtual `+0x2C8` (`0x006FDA00`), so that site keeps no
  gravity of its own.
- `BuildingClass_Mission_Missile_ApplyGravity` (`0x0044D074`, `0x0044D264`,
  `0x0044D2AE`): these sit in Mission_Missile's non-silo arm, which VERA20K
  does not port (`sim/world/techno_ai/building_missile.rs`).
- A `Gravity=` the float parse makes infinite (such as `1e39`) counts as unset.
  Phobos would carry the infinity through its x87 arithmetic.
