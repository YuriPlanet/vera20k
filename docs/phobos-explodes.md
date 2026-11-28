# Explodes.KillPassengers / Explodes.DuringBuildup

Two switches on the `TechnoClass::ReceiveDamage` death arm.

```ini
[TechnoType]                          ; any techno, buildings included
Explodes.KillPassengers=              ; Nullable<bool>, unset -> [General]

[BuildingType]
Explodes.DuringBuildup=               ; Nullable<bool>, unset -> [General]

[General]
Explodes.KillPassengers=true          ; Valueable<bool>, default true
Explodes.DuringBuildup=true           ; Valueable<bool>, default true
```

Reader `Ext/TechnoType/Body.cpp:968`, `Ext/BuildingType/Body.cpp:222`;
globals `Ext/Rules/Body.cpp:470-471`, constructors `Ext/Rules/Body.h:885-886`.

## Behavior

The stock arm of `TechnoClass::ReceiveDamage` behind `Explodes=` (veteran
`EXPLODES` or `Suicide=` weapon included) kills the object's passengers and
then fires its death weapon. Phobos `TechnoClass_ReceiveDamage_Explodes`
(`Ext/Techno/Hooks.ReceiveDamage.cpp:242-258`) replaces six bytes at
`0x702603` and returns one of two continuations:

- `0x702669` when `Explodes.KillPassengers` is false: the death weapon still
  fires, only the `FootClass::KillPassengers` loop is skipped;
- `0x702672` when the object is a Building whose `CurrentMission` is
  Construction or Selling and `Explodes.DuringBuildup` is false: both the
  passenger kill and `Fire_Death_Weapon` are skipped, i.e. the object behaves
  as if its `Explodes=` were false.

The building check reads the current mission, not the queued one.

## Where VERA20K owns it

`rules::phobos_explodes::PhobosExplodesSettings` holds both `Nullable`
values (the building key is read only for a BuildingType, as in Phobos); the
`[General]` fallbacks live in `rules::phobos_globals`. In
`world_receiver`'s death arm the `death_arm_explodes` answer is cleared for
a building under Construction/Selling when the effective DuringBuildup value
is false, and the passenger kill additionally requires the effective
KillPassengers value. The death weapon gate and the kill both read the same
two answers, matching the hook's two continuations.

RESIDUAL: Phobos documents DuringBuildup as "behave as if `Explodes` was set
to false", but the hook only redirects this tail. The debris and
`Explosion=`/`DestroyAnim=` draws are produced by the concrete receiver
after `TechnoClass::ReceiveDamage` returns and still run; VERA follows the
hook, not the documentation wording.

NOTE: the building decision reads the entity's current mission field, the
same selector native's `CurrentMission` exposes; a building killed on the
frame it leaves Construction keeps the mission it held when the damage
lands.
