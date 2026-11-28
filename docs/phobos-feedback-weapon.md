# Feedback weapon (`FeedbackWeapon`)

Phobos lets a weapon also detonate a second weapon on its own firer, once per
shot, right after `FireAt` has built the bullet and its muzzle animation.

## Keys

| Key | Owner | Type | Default |
|---|---|---|---|
| `FeedbackWeapon=` | WeaponType | `Valueable<WeaponTypeClass*>` | empty — no second detonation |
| `FireInTransport=` | WeaponType | `Valueable<bool>` | `yes` (stock key, already read by VERA) |

Reads: `WeaponTypeExt::LoadFromINIFile` (`Ext/WeaponType/Body.cpp:124`, field
`Ext/WeaponType/Body.h:56`, constructed null `:151`). The `<true>` read is
`WeaponTypeClass::FindOrAllocate`, so the named weapon joins the rules family
even when no stock type references it; VERA's ordered weapon pass follows the
reference for that reason.

## Native body

`TechnoClass_FireAt_FeedbackWeapon` (`0x6FF43F`,
`Ext/Techno/Hooks.Firing.cpp:802-812`):

```cpp
if (pThis->InOpenToppedTransport && !pWeaponFeedback->FireInTransport)
    return 0;
WeaponTypeExt::DetonateAt(pWeaponFeedback, pThis, pThis);
```

`DetonateAt(weapon, AbstractClass* target, TechnoClass* owner)`
(`Ext/WeaponType/Body.cpp:363-366` → `:374-377`) builds a bullet at
`pThis->GetCoords()` whose owner *and* target are the firer, detonates it at
once and passes no firing house. The hook sits at the end of `FireAt`'s muzzle
block (`0x006FF394..0x006FF43F`), before Sonic6FF470 and the laser ladder.

## VERA

The key is retained in `src/rules/phobos_weapon_extras.rs`
(`WeaponType::phobos_feedback_weapon`) and its reference is allocated by the
ordered rules processor.

`emit_admitted_fire` (`src/sim/combat/world_fire.rs`) fires it at the end of
the muzzle block, before Sonic and the later-effect ladder. The shot is a
`weapon_detonation_with_target` packet committed through the shared
`Simulation::commit_attach_effect_detonations` receiver path used by
`WeaponTypeExt::DetonateAt` callers, with the firer's coordinates and the
firer as owner and target. `ProjectileDetonationReason::FeedbackWeapon` marks
it as the instant `BulletExt::Detonate` shot, so it takes a single detonation
and no cluster loop. An open-topped passenger is skipped when the *feedback*
weapon's `FireInTransport=` is no.

## Residuals

- Native `BulletExt::Detonate` creates a real `BulletType` bullet at
  `Speed=100`, places it at the firer, explodes it and uninitializes it in one
  call. VERA represents that as a receiver packet at the firer's coordinates,
  so BulletType keys that only a flying bullet consumes (trajectory, laser
  trails) have no effect; the type's impact keys still apply. Trigger: a
  feedback weapon naming a `Projectile=` with such keys. Frequency:
  mod-authored. Risk: missing presentation-only flight effects.
- Retail rules author no `FeedbackWeapon=`.
