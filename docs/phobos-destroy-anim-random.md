# Unit destroy-animation random pick

`DestroyAnim.Random=` chooses between the random and the facing-based entry of
a destroyed unit's `DestroyAnim=` list.

```ini
[General]
DestroyAnim.Random=yes              ; Valueable<bool>, constructor yes

[SOMEUNIT]                          ; TechnoType
DestroyAnim.Random=                 ; Nullable<bool>, unset follows [General]
DestroyAnim=D0,D1,D2,D3,D4,D5,D6,D7
```

Readers: `Ext/Rules/Body.cpp:220` (`Body.h:664`) and
`Ext/TechnoType/Body.cpp:875` (`Body.h:145`).

## Native body

Stock `UnitClass::Destroy` picks the list entry with
`Scenario.Random.Next() % count` at `0x00738811..0x0073883B` (one draw even for
a one-entry list).

Phobos `UnitClass_Destroy_DestroyAnim` (`0x738807`,
`Ext/Anim/Hooks.AnimCreateUnit.cpp:22-29`) replaces those eight bytes with
`AnimTypeExt::ProcessDestroyAnims` (`Ext/AnimType/Body.cpp:15-43`):

- random (the default): a one-entry list takes index 0 without a draw;
  otherwise `RandomRanged(0, count-1)`.
- `no`: below eight entries index 0; an odd count of eight or more takes the
  last entry; an even count of eight or more takes
  `int(GetDir() / 256.0 * (count-1))`, where `GetDir()` is the unit's current
  primary facing as `Raw >> 8` (truncation, not the rounded direction byte).

## VERA

`ObjectType::phobos_destroy_anim_random` and
`PhobosGlobals::destroy_anim_random` carry the keys. The Unit death arm of
`sim/combat/destruction_effects.rs` applies the selection above; the stock
building arm (`0x00441CAC`, `Next() % count`) and the non-`phobos` build keep
the original pick. The random arm draws through the shared
`RandomRanged`-style kernel (`SimRng::next_range_i32_inclusive`).

## Residuals

- Only the Unit `DestroyAnim` arm is replaced. Buildings, aircraft and
  infantry keep their stock producers, which match the hook's caller.
- The facing is sampled at the death frame as the body heading's high byte
  (`body_facing_byte`, `Raw >> 8`), the same `DirType` `GetDir()` returns; it
  also feeds `CreateUnit.InheritDeathFacings`.
