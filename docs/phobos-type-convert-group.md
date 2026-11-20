# Convert pairs

A warhead detonation and a superweapon launch can each convert every Foot
they reach through a `TypeConvertGroup` list.

```ini
[WH]                                  ; Warhead
Convert0.From=                        ; list of TechnoType names, empty -> any Foot
Convert0.To=                          ; a TechnoType; unset ends the numbered list
Convert0.AffectsHouse=                ; AffectedHouse, default All for a warhead
Convert0.AffectedHouses=              ; deprecated alias, AffectsHouse wins

[SW]                                  ; SuperWeaponType
Convert0.From=                        ; the same rows, default house Owner
Convert0.To=
Convert0.AffectsHouse=

Convert.From=                         ; the unnumbered pair replaces row 0,
Convert.To=                           ; else becomes row 0
Convert.AffectsHouse=
```

Reader `New/Type/Affiliated/TypeConvertGroup.cpp:49-101` (numbered loop,
unset `To` breaks), warhead `Ext/WarheadType/Body.cpp:455` (`All`), SW
`Ext/SWType/Body.cpp:281` (`Owner`). `AffectedHouses` is Phobos's old
spelling; both keys parse through the same `Nullable<AffectedHouse>` and a
set `AffectsHouse` overrides.

## Behavior

`TypeConvertGroup::Convert` (`:4-37`), per row in list order:

1. a row without `To` is skipped;
2. when the source house is known, `CanTargetHouse(AffectsHouse, source,
   target.Owner)` must accept the target's house;
3. an empty `From` list converts the target, else the first `From` entry
   matching the target's current type name (case-insensitive) converts it;
4. the first converting row stops the list; a Foot that converted once is
   not converted again by later rows.

The conversion is `TechnoExt::ConvertToType`, the same owner as the
`Convert.*` TechnoType keys.

## Where VERA20K owns it

`rules::type_convert_group` reads the rows; `WarheadType::phobos_convert_pairs`
and `SuperWeaponType::phobos_convert_pairs` hold them.
`Simulation::phobos_apply_type_convert_pairs`
(`sim/game_entity/type_conversion.rs`) runs the native order and house test
through `combat::can_target_house`, resolves `From` against the current
interned type name, and calls `convert_techno_type`, which refuses a
same-type or cross-class `To` exactly as `TechnoExt::ConvertToType` does
(`Ext/Techno/Body.cpp:355-362`).

- Warhead `WarheadTypeExt::ApplyConvert`
  (`Ext/WarheadType/Detonate.cpp:215,669-678`): inside
  `DetonateOnOneUnit`'s per-target pass, after the
  `TechnoClass_ReceiveDamage`/shield gate and before the
  `PenetratesTransport` arm. Non-Foot targets (structures) are skipped.
  The early-return gate that skips a warhead with no crit, attach effect or
  trigger work also counts the pairs.
- Superweapon `SWTypeExt::FireSuperWeaponExt` (`:31-32`) through
  `ApplyTypeConversion` (`:349-354`): after `SW_Next` and before
  `SW_Link`, over every Foot in the global array. Ares-handler kinds reach
  the same tail at the `SuperClass_Launch` continuation
  (`Ext/SWType/Hooks.cpp:12-28`, hook `0x6CDE40`), where the launched
  PostClick data type's pairs apply.

RESIDUAL: native resolves `To`/`From` names at parse time
(`Utilities/TemplateDef.h:1728-1744,1834-1849`); an unknown `To` leaves the
`Nullable` unset (ending the numbered loop) and unknown `From` tokens are
dropped. VERA stores the names and emulates that at apply time: an
unresolvable `To` stops the list, and `From` matches only names the built
rules resolve, an all-unknown list converting any Foot. One case differs:
when `Convert0.To` is unknown but the unnumbered `Convert.To` then
replaces row 0, VERA still holds the numbered rows native never read.
Trigger: a mistyped `To` in row 0 plus an unnumbered pair; effect: a later
row can convert where native would not.

RESIDUAL: native `Parse` appends to the retained vector on every INI pass, a
later map pass adding rows after the rules rows, and its unnumbered pair
replacing `list[0]`. VERA parses the layered section once, so a map that
redefines the warhead or superweapon section replaces the whole key.
Trigger: a map INI with `Convert*` keys on a type that already has them;
effect: the map's rows replace, not extend, the rules rows.

RESIDUAL: the conversion body is the Ares-shaped `ConvertToType` in both
builds (see `docs/phobos-convert-health.md`); the different locomotion,
mission and ammo state a non-Ares `ConvertToType` would give is not
reproduced.

RESIDUAL: `TechnoExt::ConvertToType` runs the new type's `UpdateTypeData`
chain, so `Convert.ResetMindControl` and the health band apply after a pair
conversion exactly as they do for the TechnoType `Convert.*` keys.
