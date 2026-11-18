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
Crit.AffectsTarget=all              ; classes the crit may hit (see below)
Crit.AffectsHouse=all               ; house buckets the crit may hit
Crit.AffectsAbovePercent=0.0        ; health must be strictly above this
Crit.AffectsBelowPercent=1.0        ; health may equal this upper bound
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

The three target filters run inside `ApplyCrit` after the chance roll and
the immunity gates, in native order (`Detonate.cpp:553-563`):

- `Crit.AffectsTarget` is a comma list of `land`, `water`, `empty`,
  `infantry`, `units`, `buildings`, `aircraft`, `all`, `none`. The crit call
  does not consider aircraft separately, so aircraft answer to the `units`
  flag, and a 1x1 building with `UndeploysInto` (`IsStrange`) does too.
- `Crit.AffectsHouse` is a comma list of `owner`/`self`, `allies`/`ally`,
  `enemies`/`enemy`, `neutral`, `team`, `others`, `all`, `none`, matched
  against the firing house.
- `Crit.AffectsAbovePercent`/`Crit.AffectsBelowPercent` are fractions
  (`0.5` or `50%`): the target's health ratio must be strictly above the
  first and at or below the second, the interval `(above, below]`. The
  unset pair `(0.0, 1.0]` admits every live target.

The deprecated spellings `Crit.Affects`, `Crit.AffectsHouses`,
`Crit.AffectAbovePercent` and `Crit.AffectBelowPercent` read into the same
fields first; a set current key overwrites them regardless of INI order
(`WarheadType/Body.cpp:200-228`). An unknown token fails the whole enum
read and keeps the earlier value.

An AttachEffectType's own `Crit.Multiplier`, `Crit.ExtraChance`,
`Crit.AllowWarheads` and `Crit.DisallowWarheads` scale or gate the chance
while the effect is held; see [AttachEffect](phobos-attach-effect.md).

## Where VERA20K owns it

- `rules/phobos_crit.rs`: the warhead `Crit_*` block
  (`WarheadTypeExt::LoadFromINI` order), the `[CombatDamage]`/`[General]`
  defaults and the rank-ability parse (`ReadAdditionalAbilities`). The
  `AffectedTarget` parse and the house flags' `AffectedHouse` parse
  (`rules/phobos_shields.rs`) follow `Utilities/TemplateDef.h:778-874`.
- `sim/combat/crit.rs`: `GetCritChance`/`ApplyCrit`
  (`Detonate.cpp:525..608, 735..778`) inside the per-target warhead pass
  called at the entry of `BulletClass_Detonate 0x46920B` and
  `MapClass_DamageArea 0x489286`. The pass collects its own
  `getCellSpreadItemsExt` items and runs before the special/ordinary arm split,
  SpawnShrapnel and the native area damage, so a target it kills is gone when
  the main warhead dispatches. Its filter gates reuse the shared owners:
  `sim/combat/mod.rs`'s `health_in_threshold`
  (`TechnoExt::IsHealthInThreshold`, `Ext/Techno/Body.cpp:752-759`) and
  `can_target_house` (`EnumFunctions::CanTargetHouse`,
  `EnumFunctions.cpp:5-16`), and `crit.rs`'s `cell_eligible`/`techno_eligible`
  (`EnumFunctions.cpp:35-94`).
- `sim/combat/phobos_airburst.rs`: the shared `getCellSpreadItemsExt` walk.
- `rules/object_type.rs`: `ImmuneToCrit`.

## Residuals

- Of `PossibleCellSpreadDetonate`'s members VERA reads only the AttachEffect
  lists; a warhead setting only an unread member skips the two buffer draws
  native makes per detonation, shifting the scenario RNG stream.
- `CellSpread.Cylinder` **is** read and consumed: the warhead key
  (`rules/warhead_type.rs:1033`) reaches `sim::combat::combat_aoe`
  (`:661`, `:715`), which measures the cylinder instead of the sphere. The
  earlier "no reader" note here was stale.
- Phobos' `MapClass_DamageArea` hook is gated on `InDamageArea`, which a
  bullet detonation clears; VERA's direct area calls always run the pass, so a
  warhead detonated both ways can visit its targets an extra time. Pre-existing.
