# ExtraWarheads

A weapon can detonate several warheads on one impact.

```ini
[SOMEWEAPON]                              ; WeaponType
ExtraWarheads=                            ; list of WarheadType names
ExtraWarheads.DamageOverrides=            ; list of integers
ExtraWarheads.DetonationChances=          ; list of doubles (percentage or absolute)
ExtraWarheads.RollChances=                ; list of doubles (percentage or absolute)
ExtraWarheads.RandomWeightsN=             ; list of integers, one list per roll
ExtraWarheads.RandomWeights=              ; alias for RandomWeights0
ExtraWarheads.FullDetonation=             ; list of booleans
```

Reader `Ext/WeaponType/Body.cpp:139-166`. `ExtraWarheads` is a
`ValueableVector<WarheadTypeClass*>`; the consumer resolves the names (an
unknown or `none` entry is dropped, as the parse-time `Find` would).
`DetonationChances` and `RollChances` are `ValueableVector<double>` with
Phobos `Parser<double>` (binary32 store, `%` at 0.01). The numbered
`RandomWeightsN` loop stops at the first absent or empty entry, then the
unnumbered alias replaces entry 0 or appends it. Override lists share native's
"last entry past the end" rule.

## Behavior

Phobos `BulletClass_Logics_Extras` (`Ext/Bullet/Hooks.DetonateLogics.cpp:367-470`,
hook `0x469AA4`): after the bullet's own detonation, when the bullet still
remembers the fired weapon (and is still alive), each extra warhead fires at
the same impact:

- without weights, every listed warhead draws once against its
  `DetonationChances` entry (last entry for indices past the list; no list
  means a guaranteed detonation);
- with weights, one Scenario `RandomDouble` per `RollChances` entry (one
  sure roll when absent) both passes the roll and selects a weight index
  through `GeneralUtils::ChooseOneWeighted` (single-precision weight sums); a
  selected index still draws the chance dice before the bounds check rejects
  it;
- damage is the weapon's `Damage` unless `DamageOverrides` names a value;
  `FullDetonation` (default true) detonates through `WarheadTypeExt::DetonateAt`
  — a bullet with no weapon type — otherwise through
  `DamageAreaWithTarget` with the bullet's target as the intended one;
- the target, when it is a Techno, must pass the extra warhead's health,
  veterancy and invoker thresholds.

## Where VERA20K owns it

`rules::phobos_extra_warheads` reads the keys onto `WeaponType`.
`sim::combat::phobos_extra_warheads::emit` runs per cluster at the
`DetonateAtCoord` tail (`0x469AA4`, before the airburst tail `0x469EBA` and
inside the gate that skips the nullify/IronCurtain and all-map returns),
reading the weapon from the payload's `installed_weapon`, live state from the
cluster's current detonation packet, and the source house from
`detonation_source_house`; the target gate reuses only
`combat::warhead_triggers_allow` (the three trigger thresholds, without the
`CanAffectTarget` Verses/Air-Ground split); the full arm commits a
`ProjectileDetonation` packet with `ProjectileDetonationReason::ExtraWarhead`
and the area arm reuses `apply_area_damage_in_run_with_target`.

NOTE: Phobos creates AirburstWeapon children through
`BulletExt::SimulatedFiringUnlimbo`, which keeps the child's weapon pointer
(`Ext/Bullet/Body.cpp:326`), so a child weapon's own `ExtraWarheads` fire on
each child detonation; VERA's child bullets likewise carry
`installed_weapon`, and extras run for them.

`ValueableVector<WarheadTypeClass*>` resolves through
`WarheadTypeClass::FindOrAllocate` (`Utilities/TemplateDef.h:1982-1991`);
the rules projection now allocates a constructor-default `WarheadType` for
every `ExtraWarheads` token the sections omit (empty section through
`from_ini_section`, 100% `Verses`), so it keeps its index in the override
lists and detonates (`an_unknown_warhead_name_allocates_an_empty_type`); no
residual.
