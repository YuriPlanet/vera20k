# Return weapon (`ReturnWeapon`)

A detonating bullet that fires a second weapon back at the techno that fired
it, from the point of impact.

## Keys

| Key | Type | Default |
|---|---|---|
| `ReturnWeapon=` | `Valueable<WeaponTypeClass*>` (`Read<true>`) | unset |
| `ReturnWeapon.ApplyFirepowerMult=` | `Nullable<bool>` | unset |

Read: `BulletTypeExt::LoadFromINI` (`Ext/BulletType/Body.cpp:54-55`),
declarations `Ext/BulletType/Body.h:56-57`, constructor `Body.h:121-122`. VERA
reads them in `src/rules/phobos_return_weapon.rs`, owner
`ProjectileType::phobos_return_weapon`. Tests:
`src/rules/phobos_return_weapon_tests.rs`.

The reader is `Read<true>` (`Body.cpp:54`), so the named weapon type is
allocated when it is not already known; VERA keeps the name and resolves it
against `RuleSet::weapon` at detonation time.

`ReturnWeapon.ApplyFirepowerMult=` falls back to `[CombatDamage]
ReturnWeapon.ApplyFirepowerMult`, default **`no`** (`Ext/Rules/Body.cpp:559`,
constructor `Ext/Rules/Body.h:973`), which
`RuleSet::phobos_globals.return_weapon_apply_firepower_mult` carries.

> Not to be confused with `phobos_return_warhead.rs`, the Ares
> `ReturnWeapon` **warhead** that converts a dying unit into a projectile.

## Behaviour

`Ext/Bullet/Hooks.DetonateLogics.cpp:480-500`, the "Return to sender" arm of
`BulletClass_Logics_Extras` (`0x469AA4`). It runs after the `ExtraWarheads=`
block and before `UnlimboDetonate`, still inside this cluster's detonation:

```cpp
if (pTechno)                                   // pThis->Owner, the firer
{
    if (auto const pWeapon = pTypeExt->ReturnWeapon)
    {
        int damage = pWeapon->Damage;
        if (pTypeExt->ReturnWeapon_ApplyFirepowerMult.Get(
                RulesExt::Global()->ReturnWeapon_ApplyFirepowerMult))
            damage = static_cast<int>(damage * pBulletExt->FirepowerMult);

        if (auto const pBullet = pWeapon->Projectile->CreateBullet(
                pTechno, pTechno, damage, pWeapon->Warhead,
                pWeapon->Speed, pWeapon->Bright))
        {
            BulletExt::Fetch(pBullet)->FirepowerMult = pBulletExt->FirepowerMult;
            BulletExt::SimulatedFiringUnlimbo(pBullet, pOwner, pWeapon,
                                               pThis->Location, true);
            BulletExt::SimulatedFiringEffects(pBullet, pOwner, nullptr, false, true);
        }
    }
}
```

So the returned shot:

- exists only while the **firing techno** is still around (`if (pTechno)`);
- is a real bullet of the named weapon's own projectile type, carrying the
  weapon's warhead, speed and `Bright=`;
- targets **the firer itself** (`CreateBullet(pTechno, pTechno, ...)`);
- is unlimboed at the **impact coordinate** (`pThis->Location`), not at the
  firer, and heads straight at its target (`headToTarget = true`);
- takes the dying bullet's `FirepowerMult`, which also scales `Damage=` when
  the multiplier applies — `static_cast<int>` truncates toward zero and
  saturates at the `int` bounds;
- plays the weapon's **visual** effects only
  (`SimulatedFiringEffects(..., firingEffect=false, visualEffect=true)`), i.e.
  laser / electric bolt / rad beam / particle system, with **no** firing anim
  and **no** firing report.

`pOwner` is `pTechno->Owner`, falling back to `BulletExt::FirerHouse` only
when the firer is gone — which is unreachable behind the `pTechno` gate.

VERA ports this as `sim::combat::phobos_return_weapon::emit`
(`src/sim/combat/phobos_return_weapon.rs`), called from the detonation tail of
`src/sim/combat/world_receiver.rs` in the native order (extra warheads →
return weapon → `UnlimboDetonate` → airburst). It reuses
`BulletTypeExt::CreateBullet` + `SimulatedFiringUnlimbo`'s child construction
(`super::phobos_airburst::simulated_velocity`, `FiredBullet::launched`,
`admit_fired_bullet`) and the visual half of
`super::phobos_airburst::admit_firing_effects`, which the airburst emitter
shares.

## Residuals

None recorded. The returned weapon is fired at the impact coordinate through the stock bullet path,
and only the type name is deferred to load-time resolution.

## Tests

- `src/rules/phobos_return_weapon_tests.rs` — the pair of type keys, the
  `[CombatDamage]` fallback, and the `<default>` / `<none>` reset tokens.
- `src/sim/combat/phobos_return_weapon_emission_tests.rs` —
  `Damage=` scaling (`static_cast<int>` truncation, saturation), and the
  type-key → global-fallback chain through `ProjectileType` and
  `PhobosGlobals`.
