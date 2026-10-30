# Optional Phobos critical hits

Build with `--features phobos` to give warheads a critical-hit chance: a hit
can roll extra damage, fire a second warhead at the target and play an
animation. Default builds never roll a critical.

```ini
[WarheadType]                       ; rulesmd.ini
Crit.Chance=5%                      ; per-hit chance (also an absolute number)
Crit.ExtraDamage=50                 ; extra damage on a critical
Crit.ExtraDamage.ApplyFirepowerMult=no
Crit.Warhead=                       ; a second warhead detonated on the target
Crit.Warhead.FullDetonation=no      ; full cell spread instead of one target
Crit.AnimList=                      ; animations, one plays on a critical
Crit.AnimList.PickRandom=no         ; pick one instead of the first alive
Crit.AnimList.CreateAll=no          ; create every listed animation
Crit.AnimOnAffectedTargets=no       ; play on each affected target instead
Crit.ActiveChanceAnims=             ; anims shown while the chance is armed
Crit.ApplyChancePerTarget=no        ; [CombatDamage]
Crit.SuppressWhenIntercepted=no     ; [CombatDamage]
ApplyPerTargetEffectsOnDetonate=no  ; [CombatDamage]

[TANK]
ImmuneToCrit=no                     ; TechnoType never takes a critical
VeteranAbilities=CRITCHANCE         ; rank ability: +[General] VeteranCritChance
EliteAbilities=CRITCHANCE           ; (RELOAD/EMPTY_RELOAD/RANGE/CRITIMMUNE parse too)

[General]
VeteranCritChance=0.05              ; the rank ability's per-rank bonus
```

`CRITCHANCE` is the rank-ability token that raises the carrier's critical
chance by `[General] VeteranCritChance` per rank; `CRITIMMUNE`, `RELOAD`,
`EMPTY_RELOAD` and `RANGE` are parsed alongside, but only `CRITCHANCE` and
`CRITIMMUNE` have consumers in this chain.

An AttachEffectType's own `Crit.Multiplier`, `Crit.ExtraChance`,
`Crit.AllowWarheads` and `Crit.DisallowWarheads` scale or gate the chance
while the effect is held; see [AttachEffect](phobos-attach-effect.md).

## Where VERA20K owns it

- `rules/phobos_crit.rs`: the warhead `Crit_*` block
  (`WarheadTypeExt::LoadFromINI` order), the `[CombatDamage]`/`[General]`
  defaults and the rank-ability parse (`ReadAdditionalAbilities`).
- `sim/combat/crit.rs`: `GetCritChance`/`ApplyCrit`
  (`Detonate.cpp:525..608, 735..778`) inside the per-target warhead pass
  called from `BulletClass_Detonate 0x46920B` and
  `MapClass_DamageArea 0x489286`. The pass runs before the native area
  damage, so a target it kills is gone when the main warhead dispatches.
- `rules/object_type.rs`: `ImmuneToCrit`.

## Residuals

- The per-target pass runs only in the ordinary-damage arm of bullet
  detonation and in `DamageArea`; Phobos runs it before the whole bullet arm,
  so a warhead on a special arm (Ivan bomb, mind control, temporal,
  locomotor, ...) skips its critical roll and prefix draws.
- Of `PossibleCellSpreadDetonate`'s members VERA reads only the AttachEffect
  lists; a warhead setting only an unread member skips the two buffer draws
  native makes per detonation, shifting the scenario RNG stream.
- The per-target list reuses the area receiver collection instead of
  Phobos's `getCellSpreadItemsExt` walk, so membership and order can differ
  (no airborne phase, different cell filters).
