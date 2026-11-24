# Warhead trigger conditions

Phobos adds per-warhead health, veterancy and invoker gates to the target list
of a detonation (`WarheadTypeExt::CanAffectTarget`,
`Ext/WarheadType/Body.cpp:37-57`).

```ini
[WH]                                  ; WarheadType
AffectsAbovePercent=0.0               ; double, default 0.0
AffectsBelowPercent=1.0               ; double, default 1.0
AffectsVeterancy=all                  ; AffectedVeterancy, default all
AffectsInvokerOnly=no                 ; boolean, default no
AffectsInvokerOnly.Reverse=no         ; boolean, default no
AffectsInvokerOnly.IgnoreInvokerState=; unset inherits [CombatDamage]
```

```ini
[CombatDamage]                        ; rulesmd.ini
AffectsInvokerOnly.IgnoreInvokerState=yes  ; boolean, default yes
Shrapnel.ObeyWarheadTriggerConditions=yes  ; boolean, default yes
```

```ini
[Comment]                             ; BulletType section
Shrapnel.ObeyWarheadTriggerConditions=; Nullable, unset inherits [CombatDamage]
```

## Behavior

The gates are read at three native sites: `DetonateOnOneUnit`'s target pass
(`Ext/WarheadType/Detonate.cpp:203`, the crit/laser/attach-effect list), the
entry of `TechnoClass::ReceiveDamage` (`Ext/Techno/Hooks.ReceiveDamage.cpp:26-34`),
where a refused packet is zeroed, and `BulletClass_Logics`'s detonation entry
(`0x4690D4`, `Ext/Bullet/Hooks.DetonateLogics.cpp:9-41`). The receive site runs
before the shield and does not set `SkipLowDamageCheck`, so native's
`0x7019D8` floor still turns that zero packet into one point of damage; the
bullet site, however, skips the whole body first while its key is on, so a
refused bullet detonation produces no packet and the floor does not apply.
With the bullet key off the body runs and the receive site's zero-and-floor
path still gives one point. A target is skipped unless it passes, in native
order:

- **Health**: only checked when `AffectsAbovePercent > 0` or
  `AffectsBelowPercent < 1` (Phobos's derived `HealthCheck`); the target's
  current-health ratio must lie in `(above, below]`, with the zero-health,
  zero-strength object matching unconditionally.
- **Veterancy**: only checked when the mask is not `all` (derived
  `VeterancyCheck`); the target's rank bit must be in `AffectsVeterancy`.
- **Invoker**: with `AffectsInvokerOnly`, a present invoker (the techno that
  caused the damage) must be — or with `.Reverse`, must not be — the target.
  Without an invoker, `.IgnoreInvokerState` or the `[CombatDamage]` global
  (constructing `yes`) decides whether the target is affected.

Phobos reads the two percentages through its `Valueable<double>` reader, which
narrows the parsed value through f32 as Phobos's INI wrapper does.

### Bullet detonation gate

`Shrapnel.ObeyWarheadTriggerConditions` (BulletType `Nullable`, `[CombatDamage]`
global default **yes**) gates the direct Techno target of a bullet at
`0x4690D4`. When the effective value is on and the target fails health,
veterancy, the `AffectsNeutral` filter or `IsInvokerAllowed`, the hook jumps to
`GoToExtras 0x469AA4`: the shake RNG draws, radiation, the Phobos per-target
pass (crit, convert, return warhead, ...), the stock damage arms and the area
damage are all skipped, while the impact extras (the warhead `AnimList` and
its smudge, debris and `ExtraWarheads`) still run. Only the
`DetonateOnAllMapObjects` claim precedes `0x4690D4`; radiation (`0x469150`),
the interceptor head (`0x46920B`) and the 0x46920B per-target pass follow the
gate and are skipped with the body. A cell, terrain or null target never takes
the gate; only an entity target does.

## Where VERA20K owns it

`rules::warhead_type` reads the keys; `sim::combat` owns the three predicates
and `warhead_triggers_allow`, which `WarheadTypeExt::CanAffectTarget`'
`detonate_on_one_unit` consumer uses (`sim::combat::crit`) before the crit
pass. The same compiler flags are shared with the splits/shrapnel
weapon-targeting gates. `rules::projectile_type` reads the bullet key and
`rules::phobos_globals` the `[CombatDamage]` fallback;
`sim::combat::world_receiver::warhead_trigger_skips` runs the 0x4690D4 gate in
the detonation transaction, before the shake draws and the receiver body, and
returns to the existing extras block.

## Residuals

- The receive hook's `ApplyPerTargetEffectsOnDetonate` arm (which calls
  `DetonateOnOneUnit` from ReceiveDamage) is not modelled; only the trigger
  gate of that hook is.
- Radiation is queued on `CombatEmit` while interception resolves its prey
  immediately, so the radiation/interceptor order has no observable case; the
  interceptor regression guards the gate skip itself, not that order.
- `CanAffectTarget`'s later `EffectsRequireVerses`, `AffectsAir`/`AffectsGround`
  gates are already modelled separately and keep their own owner.
