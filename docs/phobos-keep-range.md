# KeepRange

An Infantryman or Unit keeps its distance while its weapon recharges.

```ini
[SOMEWEAPON]                          ; WeaponType
KeepRange=0                           ; Leptons (cells in INI), read as |value|
KeepRange.AllowAI=                    ; Nullable<bool>, unset -> [General]
KeepRange.AllowPlayer=                ; Nullable<bool>, unset -> [General]
KeepRange.EarlyStopFrame=             ; Nullable<int>, unset -> [General]

[General]
KeepRange.AllowAI=false               ; Valueable<bool>
KeepRange.AllowPlayer=false           ; Valueable<bool>
KeepRange.EarlyStopFrame=0            ; Valueable<int>
```

Reader `Ext/WeaponType/Body.cpp:179-182`; defaults `Ext/WeaponType/Body.h:182-185`;
globals `Ext/Rules/Body.cpp:269-271`, `Ext/Rules/Body.h:187-189`.

## Behavior

`WeaponTypeExt::GetTechnoKeepRange` (`Ext/WeaponType/Body.cpp:445-507`), read
by `TechnoClass::InRange`'s maximum and minimum range getters
(`Ext/Techno/Hooks.WeaponRange.cpp:134-145`, and the obstacle probe at
`Ext/Bullet/Hooks.Obstacles.cpp:209-215`):

- nothing happens for a zero `KeepRange=`, no firer, a transported firer, a
  category other than Infantry or Unit, or a house whose AllowAI/AllowPlayer
  key (type value, else the `[General]` fallback) is off;
- the rearm countdown must have more time left than `EarlyStopFrame`;
- a positive value becomes the minimum range (maximum falls through);
- a negative value becomes the maximum range, except that a target at or
  beyond `-value - 128` leptons makes it `max(-value - 128, 443)` so the
  firer closes in instead of stopping.

## Native anchors

`TechnoClass::InRange` (`0x006F7220`) is the predicate the keys modify; Phobos hooks its weapon-range
arm at `0x6F7248` (`Ext/Techno/Hooks.WeaponRange.cpp:131`) and its minimum-range arm at `0x6F737F`
(`Ext/Bullet/Hooks.Obstacles.cpp:206`).

## Where VERA20K owns it

`WeaponType::phobos_keep_range*` and the three `phobos_globals` values feed
`in_range::keep_range_overrides`, applied to a local weapon copy inside
`in_range_at_slot` before the reach/minimum gates: it raises
`minimum_range_leptons` (positive) or replaces `range_leptons` (negative).

RESIDUAL: native's `GetTechnoKeepRange` also allows the override when the
rearm timer is not running but the firer's SpawnManager is in its cooldown
state and no spawned node is taking off or returning; VERA's spawn manager
does not model that node status pair, so a carrier in that state gets no
override. Trigger: a `Spawner=` Infantry/Unit with `KeepRange=`; effect: it
does not hold the distance while its spawns are between sorties.

RESIDUAL: the negative branch compares against the firer's assigned target
(`pFirer->Target`); VERA measures the InRange query's target entity. They
coincide for the fire/close-enough checks this ports; a query about another
target uses that target's distance.

A negative `KeepRange` replaces the maximum-range getter, so the native path
never adds the AttachEffect `WeaponRange.*` modifiers to it; VERA's
`in_range_at_slot` likewise uses the plain `keep` value instead of
`weapon_range_with_modifiers` when `keep_max` is set. No residual.

RESIDUAL: `mapless_in_range` (the headless/pre-map policy path) does not apply
the override; production always has a resolved map.
