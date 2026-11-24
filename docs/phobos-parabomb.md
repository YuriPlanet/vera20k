# Parabombs (`Parachuted`, `BombParachute`)

A `BulletType` whose projectile is born under a canopy and descends to the
ground instead of flying its trajectory.

## Keys

| Key | Type | Default |
|---|---|---|
| `Parachuted=` | `Valueable<bool>` | `no` |
| `Parachuted.FallRate=` | `Valueable<int>` | `1` |
| `Parachuted.MaxFallRate=` | `Nullable<int>` | unset |
| `BombParachute=` | `Nullable<AnimTypeClass*>` | unset |

Read: `BulletTypeExt::LoadFromINI` (`Ext/BulletType/Body.cpp:79-82`),
declarations `Ext/BulletType/Body.h:83-86`, constructor `Body.h:145-148`. VERA
reads them in `src/rules/phobos_parabomb.rs`, owner
`ProjectileType::phobos_parabomb`. Tests:
`src/rules/phobos_parabomb_tests.rs`.

An unset `Parachuted.MaxFallRate=` falls back to `[General] ParachuteMaxFallRate`
and an unset `BombParachute=` to `[General] BombParachute` (`Rules+0xBB8`), both
of which the `RulesClass` owner VERA already has.

### Nullable semantics

`Nullable<T>::Read` (`Utilities/TemplateDef.h:1833-1849`) resets on
`<default>`; a blank value never reaches the reader (`ReadString` answers 0)
and therefore keeps whatever the caller had. `Nullable<AnimTypeClass*>` also
accepts the type-pointer sentinel `none`/`<none>`, which VERA's
`read_phobos_nullable_with` folds into the same reset.

## Descent rate

`Ext/Bullet/Hooks.cpp:128-137` (the `BulletClass::AI` tail):

```cpp
if (pThis->HasParachute)
{
    int fallRate = pBulletExt->ParabombFallRate - pBulletTypeExt->Parachuted_FallRate;
    const int maxFallRate = pBulletTypeExt->Parachuted_MaxFallRate
        .Get(RulesClass::Instance->ParachuteMaxFallRate);
    if (fallRate < maxFallRate)
        fallRate = maxFallRate;
    pBulletExt->ParabombFallRate = fallRate;
    pThis->FallRate = fallRate;
}
```

So `Parachuted.FallRate=` is the per-frame **acceleration** (a positive step
subtracted from a rate that is negative while descending) and
`Parachuted.MaxFallRate=` / `[General] ParachuteMaxFallRate` the **terminal**
rate — a signed lower bound, i.e. the fastest the canopy may fall.
`ParabombFallRate` starts at `0` (`BulletExt::ParabombFallRate`,
`Ext/Bullet/Body.h:61`), so the first frame computes `0 - step` and the cap
holds the rate at the terminal value whenever the step is positive.

VERA owns this formula as
`sim::movement::parachute_descent::parachuted_bullet_rate`
(`src/sim/movement/parachute_descent.rs`), the same step/cap shape
`ObjectFallState::advance_rate` uses for a parachuting **object** — only the
owners of `step` and `cap` differ. Tests:
`src/sim/movement/parachute_descent.rs`'s `parabomb_rate_tests`.

## Behaviour

Three separate native arms make a parabomb:

1. **`Unlimbo` becomes `SpawnParachuted`** (`Ext/Bullet/Hooks.cpp:573-583`,
   hooking `0x468684`): a `Parachuted=yes` bullet is born under its canopy
   rather than flying. `BulletClass_Unlimbo_Obstacles`
   (`Ext/Bullet/Hooks.Obstacles.cpp:136-142`) clears its velocity.
2. **The canopy anim** (`:585-608`, hooking `0x5F5A62` inside
   `ObjectClass::SpawnParachuted`): `BombParachute.Get([General] BombParachute)`
   constructs an `AnimClass` at the spawn coordinate, owned by the firing house
   and remapped to its colour scheme.
3. **The vanilla motion is skipped** (`:610-618`, hooking `0x467AB2`): when
   `HasParachute`, the `0x467AB2..0x467B1A` ordinary gravity/reflection commit
   is bypassed outright and the descent is driven purely by `FallRate`.

VERA consumes all three. `sim::world::Simulation::init_parabomb` runs from the
Fire admission after the bullet type and firer house are known: it stores
`Projectile::parabomb` (`ParabombState`: accumulated rate, `Parachuted.FallRate=`
step and the resolved terminal rate), zeroes the velocity, keeps the Fire
coordinate, and constructs the `BombParachute` (else `[General] BombParachute`)
AnimClass through the shared anim owner with the firing house's remap. The
bullet AI's parachute arm
(`sim::projectile::ProjectileStore::advance_after_head`) accumulates the rate
with `sim::movement::parachute_descent::parachuted_bullet_rate`, descends Z,
skips the ordinary collision/gravity tail, and detonates when the new Z reaches
the ground.

`[General] BombParachute=` is published by the ordered rules type-reference
owner as `RuleSet::general.bomb_parachute`; an unset type key reads it.

## Residual

- **Canopy lifetime.** Native stores the bullet's canopy pointer; VERA lets
  the AnimClass play at its constructed coordinate and retire on its own. The
  canopy never follows the falling bullet in either implementation.
- **`Inviso=` combination.** An `Inviso=yes` + `Parachuted=yes` type would
  have its placement re-taken by the Inviso arm; no retail bullet combines
  them.

## Tests

- `src/rules/phobos_parabomb_tests.rs` — the four keys, the
  `[General] ParachuteMaxFallRate` fallback, and the `<default>` / `none`
  reset tokens.
- `src/sim/movement/parachute_descent.rs` `parabomb_rate_tests` — the
  step/cap descent, including the saturating and overflow shapes.
- `src/sim/projectile.rs` `a_parabomb_descends_by_its_fall_rate_and_grounds` —
  the vertical descent, the terminal-rate cap, the skipped trajectory and the
  ground detonation.
- `src/sim/combat/phobos_bullet_init_tests.rs`
  `a_parachuted_bullet_type_starts_its_descent_at_the_general_cap` — the Fire
  admission state and the `Parachuted.MaxFallRate=` fallback, and
  `a_parachuted_bullet_spawns_its_bomb_parachute_canopy` — the
  `BombParachute=` canopy at the spawn coordinate with the firing House's
  remap; `an_unset_parachuted_type_keeps_the_straight_trajectory` — an unset
  `Parachuted=` admits no descent state and no canopy.
