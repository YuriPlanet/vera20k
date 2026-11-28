# Anim Damage.ApplyFirepowerMult

An animation's loop damage can inherit the firepower multiplier of the
Techno that spawned it, so a veteran or crate-boosted unit's scorch or
explosion hits as hard as its weapon would.

```ini
[PULSE]                        ; Animation ART section
Damage.DealtByInvoker=yes      ; must be set for the fold to run
Damage.ApplyFirepowerMult=yes  ; Nullable<bool>, unset -> the global

[CombatDamage]
AnimDamage.ApplyFirepowerMult=no  ; Valueable<bool>, default no
```

Readers: the type key at `Ext/AnimType/Body.cpp:98`; the global at
`Ext/Rules/Body.cpp:571` (constructor `Ext/Rules/Body.h:983` stores false).

## Native body

`AnimClass_AI_Damage` (`0x42453E`, `Ext/Anim/Hooks.cpp:41-140`):

- Inside the `Damage.DealtByInvoker` block and after its `appliedDamage > 0`
  gate, when the anim has an invoker, the first damage round
  (`Animation.Value == max(Damage.Delay - 1, 1)`) caches
  `pExt->FirepowerMult = TechnoExt::GetCurrentFirepowerMultiplier(pInvoker)`.
- Every positive applied damage is then
  `(int)(appliedDamage * pExt->FirepowerMult)`, before the
  `Weapon=`/warhead dispatch. `FirepowerMult` is constructed 1.0
  (`Ext/Anim/Body.h:62`), so visits before the first damage round and a
  no-invoker anim are unscaled (identity).
- Without `Damage.DealtByInvoker` the block does not run, so the multiplier
  stays 1.0.

`GetCurrentFirepowerMultiplier` composes the object's crate firepower, the
house difficulty/country bias, the AttachEffect firepower term, the
`FIREPOWER` veteran ability, and the occupied/bunker/open-topped stages.
VERA's owner for that whole composition is
`combat::world_fire::bullet_init_firepower_multiplier` (the same helper the
bullet payload uses).

## VERA

`CombatDamageDefaults::anim_damage_apply_firepower_mult` and
`AnimTypeRuntimeConfig::damage_apply_firepower_mult` read the key pair.
`AnimObject::phobos_firepower_mult` (serialized, hashed when not 1.0) caches
the multiplier on the first damage round, only for a positive application;
the anim damage owner folds each positive application (including the
`Weapon=` detonation damage) with `attach_effect::scale_i32`, the shared
model of the native cast.

## Tests

`src/sim/anim_damage_tests.rs::anim_damage_applies_the_invokers_firepower_multiplier`
drives a real anim against a target with an invoker carrying a 2x crate
multiplier: the keyed run's damage equals `scale_i32(base, multiplier)` while
the unset key and the no-`DealtByInvoker` runs stay at the base damage, and
the `[CombatDamage]` global supplies an unset type key. The fold was mutated
red and restored.

## Residuals

- The cache is refreshed only on a positive application whose
  `Animation.Value` equals `max(Damage.Delay-1, 1)`; if that visit deals no
  damage (a delay larger than the animation length), the multiplier stays
  1.0, as natively.
- The delayed `Weapon=` dispatch keeps `RAD_NO_ATTACKER` as the bullet's
  attacker where Phobos passes the resolved invoker; that is a pre-existing
  residual of the shared `Damage.Delay`/`Weapon=` owner, not of this key.
- The multiplier helper is the one the bullet payload uses; any residual of
  that composition (for example a stage the retail data never reaches)
  applies to both consumers.
