# Rocker warhead keys

A warhead adjusts the area rocker's amplitude.

```ini
[Warhead]
Rocker.AmplitudeMultiplier=1.0     ; Valueable<double>
Rocker.AmplitudeOverride=          ; Nullable<int>
```

Reader `Ext/WarheadType/Body.cpp:188-189`, declarations
`Ext/WarheadType/Body.h:59-60`, defaults `:338-339`; consumer
`MapClass_DamageArea_Rocker` (`0x489B49`,
`Ext/WarheadType/Hooks.cpp:395-410`).

## Behavior

- The native `MapClass::DamageArea` rocker base is
  `damage * 0.01`, capped at 4.0 (`0x489B49..81`). Phobos replaces the base
  before the cap: `rocker = AmplitudeOverride.Get(damage)`; then
  `rocker *= 0.01 * AmplitudeMultiplier` in C++ double arithmetic.
- An explicit override of 0 yields no rocker (the post-cap 0.3 weak-force
  gate skips); the multiplier also scales an override.
- The result feeds the same area-rocker path (`area_rocker.rs`) for every
  in-radius Techno.

## Residuals

None recorded. The new keys feed the single area-rocker owner, and the existing native oracle still
matches with the default keys.

## Where VERA20K owns it

`WarheadType::phobos_rocker_amplitude_*` feed
`sim::combat::world_receiver::area_rocker::force`, the single owner of the
native amplitude, cap and weak-force gate. The existing native corpus
(`tools/projectile_oracle/area_rocker.json`) still matches with the default
keys; the new test pins override and multiplier values.
