# Ammo auto-convert

A Foot converts to another type when its ammo enters a band.

```ini
[SOMETECHNO]                          ; TechnoType
Ammo.AutoConvertType=                 ; TechnoType, absent/none disables
Ammo.AutoConvertMinimumAmount=-1      ; Valueable<int>
Ammo.AutoConvertMaximumAmount=-1      ; Valueable<int>
```

Reader `Ext/TechnoType/Body.cpp:1153-1158` (the reversed-pair developer
warning at `:1157`); constructor defaults `Ext/TechnoType/Body.h:794-796`.

## Behavior

`FootExt::AmmoAutoConvertActions` (`Ext/Foot/Body.cpp:888-909`), the
`FootClass_AI` tail step between the tiberium eater and the health-band arm:

- an unset `Ammo.AutoConvertType`, or both bounds negative, does nothing;
- the TYPE's own `Ammo=` (capacity) must be above zero;
- the instance's ammo must satisfy `(min < 0 || ammo >= min) &&
  (max < 0 || ammo <= max)` (one negative bound is unbounded; a reversed
  pair is an empty band, which Phobos warns about but does not swap);
- the object converts through the shared type-conversion owner.

## Native anchors

`FootExt::AmmoAutoConvertActions` (`Ext/Foot/Body.cpp:888..909`) is the tail step of the `FootClass_AI`
visit Phobos hooks at `0x4DA54E`, between the tiberium eater and the health-band arm.

## Where VERA20K owns it

Fields live on `rules::phobos_convert::PhobosConvert`, next to the other
`Convert.*` keys. `Simulation::phobos_ammo_auto_convert` in
`sim/game_entity/type_conversion.rs` reads the old type's settings and the
instance ammo, applies the band and calls the shared `convert_techno_type`.
It runs from the `FootClass_AI` tail (`world/techno_ai.rs`) before the
health-band conversion, matching the native order.

RESIDUAL: `Ammo.AutoConvertType=` resolves through the parse-time
`Nullable<TechnoTypeClass*>` `Find` natively (an unknown or forward name
stays unset and a later pass keeps the earlier valid value). VERA stores the
name and resolves at call time: an unknown name converts nothing, and a
later pass naming an unresolvable type drops the earlier valid target.
Trigger: layered rules with a misspelled or forward target; effect: the
conversion native retains can be absent in VERA.

RESIDUAL: an absent instance ammo state reads as 0; production constructs
ammo for every Foot, so only fixtures and legacy saved states can diverge.
