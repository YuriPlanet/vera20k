# Cylinder rangefinding

`CylinderRangefinding` makes a techno measure direct weapon range in the plan
(`dx² + dy²`) instead of three dimensions, so a target's altitude does not
shorten the reach.

```ini
[General]
CylinderRangefinding=no      ; default

[WEAPON]
CylinderRangefinding=yes     ; Nullable<bool>; unset falls back to [General]
```

## Native evidence

`0x6F755A` (`Ext/Techno/Hooks.Firing.cpp:1265-1274`) replaces the branch in
`TechnoClass::IsCloseEnough`:

```
cylinder = WeaponTypeExt::CylinderRangefinding.Get(RulesExt::Global()->CylinderRangefinding)
return (cylinder || pThis->WhatAmI() == Aircraft) ? 0x6F75B2 : 0x6F7568
```

`0x6F75B2` is the planar `dx² + dy²` arm; `0x6F7568` is the 3-D arm. A false
type value is not "disable": it falls back to the global, and only `false`
both places leaves a ground techno on the 3-D arm. Aircraft keep the planar
arm regardless.

Readers: `[General] CylinderRangefinding=` (`Ext/Rules/Body.cpp:525`,
constructor `Body.h:442`) and `[WeaponType] CylinderRangefinding=`
(`Nullable<bool>`, `Ext/WeaponType/Body.cpp:207`).

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::cylinder_rangefinding` (default
  `false`).
- `rules::weapon_type::WeaponType::phobos_cylinder_rangefinding`.
- `sim::combat::in_range::direct_reach_is_planar` resolves the weapon value
  over the global and ORs the stock aircraft arm
  (`sim/combat/in_range.rs:208`). Both `direct_reaches` call sites
  (`:751`, `:1370`) pass the weapon and rules.

## Residuals

- The arcing arm (`0x6F7404..0x6F7519`) has its own 2-D approximation and is
  untouched by this key, matching native.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
