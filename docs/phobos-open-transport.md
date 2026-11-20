# OpenTransport keys

A passenger in an open-topped carrier adjusts its own firepower, range and
fire-while-moving gate.

```ini
[TechnoType]                          ; the passenger
OpenTransport.RangeBonus=             ; Nullable<int>, cells
OpenTransport.DamageMultiplier=       ; Nullable<float>
OpenTransport.FireWhileMoving=        ; Nullable<bool>

[CombatDamage]
OpenTransport.RangeBonus=0            ; Valueable<int>
OpenTransport.DamageMultiplier=1.0    ; Valueable<float>

[General]
OpenTransport.FireWhileMoving=yes     ; Valueable<bool>
```

Reader `Ext/TechnoType/Body.cpp:890-892`, declarations
`Ext/TechnoType/Body.h:160-162,557-559`; globals `Ext/Rules/Body.cpp:463-465`,
defaults `Ext/Rules/Body.h:878-880`.

## Behavior

- `TechnoClass_IsCloseEnoughToTarget_OpenTopped_RangeBonus` (`0x6F72D2`,
  `Ext/Techno/Hooks.Transport.cpp:167-183`): the carrier's
  `OpenTopped.RangeBonus` (else the YR `[CombatDamage]` global) plus the
  passenger's `OpenTransport.RangeBonus` (else the Phobos global), in cells.
- `TechnoClass_FireAt_OpenToppedDmgMult` (`0x6FE43B`,
  `Ext/Techno/Hooks.Firing.cpp:714-741`) and
  `GetCurrentFirepowerMultiplier` (`Ext/Techno/Body.cpp:228-232`): the
  carrier's `OpenTopped.DamageMultiplier` times the passenger's
  `OpenTransport.DamageMultiplier` (each falling back to its global).
- `TechnoClass_CanFire_OpenTopped` (`0x6FC5C7`,
  `Ext/Techno/Hooks.Firing.cpp:439-482`): the fire-while-moving gate is the
  AND of the carrier's `OpenTopped.FireWhileMoving`, the passenger's
  `OpenTransport.FireWhileMoving` and the weapon's YR `FireWhileMoving`.

## Where VERA20K owns it

`rules::phobos_open_transport::PhobosOpenTransport` on `ObjectType`, the three
`[CombatDamage]`/`[General]` fallbacks in `rules::phobos_globals`, consumed by
`sim::combat::in_range::open_topped_range_bonus_leptons`,
`sim::combat::world_fire::open_topped_damage_multiplier` and the
`FirerFacts::passenger_fire_while_moving` term in `sim::combat::fire_error`.

RESIDUAL — `FireWhileMoving` transport-order gate `0x4D9510`: trigger = an
open-topped carrier whose passenger rushes a target while the carrier,
passenger or weapon `FireWhileMoving` AND is false; impact = the passenger's
stale `LocomotorTarget`/`TemporalImUsing` locks are not released; frequency =
rare (chrono/magnetron passenger on a moving carrier); risk = low, no wrong
damage or range. The `0x6FC5C7` GetFireError gate above is modeled.
