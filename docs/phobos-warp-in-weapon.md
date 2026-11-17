# Warp-in weapon

A Teleport locomotor's warp-in animation step can detonate a weapon at the
arrived object. Phobos adds the arm to
`TeleportLocomotionClass_ILocomotion_Process_WarpInAnim @ 0x719742`
(`Ext/TechnoType/Hooks.Teleport.cpp:74-98`); the weapon keys are read in
`Ext/TechnoType/Body.cpp:870-873`, the type threshold at `Body.cpp:865`; the
weapon defaults are `Ext/TechnoType/Body.h:534-537`, the threshold's unset
default is `Body.h:530`.

```ini
[TECHNO]                             ; TechnoType
WarpInWeapon=                        ; WeaponType name, default none
WarpInMinRangeWeapon=                ; WeaponType name, default none
WarpInWeapon.UseDistanceAsDamage=false ; boolean, default no
ChronoRangeMinimum=                  ; nullable int, type override
```

The two weapon keys are `Valueable<WeaponTypeClass*>` with `Read<true>`: the
named weapon is constructed and read on the same rules pass, even if no
`[WeaponTypes]` registry lists it. A blank value or `none` names no weapon.
`ChronoRangeMinimum` is the TechnoType's nullable override read at
`Ext/TechnoType/Body.cpp:865`.

## Behavior

After the arrival warp-in animation constructs and after the owner has landed:

1. `FootExt::LastWarpDistance` (leptons) is compared with the arriving
   TechnoType's `ChronoRangeMinimum` (`Body.cpp:865`), falling back to
   `[General] ChronoRangeMinimum` while the type key is unset. Below it,
   `WarpInMinRangeWeapon` is used, falling back to `WarpInWeapon` while unset;
   at or above it only `WarpInWeapon` applies.
2. The chosen weapon detonates once at the object's `GetCoords`, with the
   object as both target and owner (`WeaponTypeExt::DetonateAt(weapon,
   pLinked, pLinked, damage)`). The area damage sees the object as its own
   source, so its warhead still needs `AllowDamageOnSelf=yes` or the type's
   `DamageSelf=yes` to hurt the teleporter, exactly as any other self-hit.
3. `WarpInWeapon.UseDistanceAsDamage` replaces the weapon's `Damage=` with
   `LastWarpDistance / 256` (integer division). Otherwise the weapon's own
   `Damage=` is used.

The arm belongs to the ordinary Teleport Process only. The Chronosphere's
separate warp-in step (`0x719B1E`, `Chronoshift_WarpIn`) has no weapon arm in
Phobos, and `WarpOutWeapon` (`Body.cpp:872`) is a different key.

## Where VERA20K owns it

Rules live in `rules::phobos_warp_in_weapon` and the type's
`ObjectType::phobos_warp_in_weapon`; the same-pass `Read<true>` allocation is
`native_processing::allocate_warp_in_weapon_references` over every TechnoType
family. The warp distance is stored on `TeleportState` when the warp arms
(`arm_teleport`, from the same owner-GetCoords-vs-destination measure the
chrono delay uses), because the owner stands at the departure point until the
relocation. `sim::movement::teleport_movement::apply_warp_in_weapon` selects
the weapon and detonates it through the existing
`sim::kill_weapon::weapon_detonation_at` /
`commit_attach_effect_detonations` owner, called from the object turn right
after the arrival `WarpOut` animation (`sim::world::object_turn`).

## Residuals

- `LastWarpDistance` is VERA's shared `Sqrt_Approx` i32 len (the chrono-delay
  kernel), while native stores a double `Math::sqrt`. The two can differ by a
  lepton at rounding boundaries, which changes `distance / 256` only when the
  value sits on a cell boundary.
- The distance is recorded when the warp arms rather than at the departure
  warp step. The ordinary adapter does not move the owner between those two
  points, so the value is the same; a path that changed the owner's coordinates
  while a warp was armed would differ.
- `WarpInWeapon` is a weapon/object detonation owner, not a second bullet
  system: projectile type, warhead, house logic and self-damage all run through
  the ordinary detonation receivers.
