# Berzerk targeting

`[CombatDamage] BerzerkTargeting=` restricts the houses a berzerk firer may
target. It is an `AffectedHouse` list (`owner`, `allies`, `enemies`,
`neutral`, `team`, `others`, `all`, `none`), default `all`, read by
`Ext/Rules/Body.cpp:420` (`Valueable<AffectedHouse>`, constructor
`Ext/Rules/Body.h:846`).

```ini
[CombatDamage]
BerzerkTargeting=enemies   ; a berzerk unit shoots only enemy houses
```

## Native evidence

`Ext/Techno/Hooks.Firing.cpp:370-371`, in the `pTargetTechno` block right
after the iron-curtain arm:

```cpp
if (pThis->Berzerk && !EnumFunctions::CanTargetHouse(
        RulesExt::Global()->BerzerkTargeting, pThis->Owner, pTargetTechno->Owner))
    return CannotFire;
```

`CanTargetHouse` answers `owner` when the two houses are the same, `neutral`
for a `MultiplayPassive` target house, then `allies` or `enemies` by the
alliance graph; `all` admits everything. Only a techno target reaches the
arm; cells and terrain objects skip it.

## Where VERA20K owns it

- `rules::phobos_globals::PhobosGlobals::berzerk_targeting`, read with
  `phobos_shields::read_affected_house` from `[CombatDamage]`.
- `sim::combat::fire_error::FireFacts::berzerk_targeting`, carried on
  `fire_error_world::FirerPart` from the rules globals.
- `sim::combat::can_target_house` (the shared `CanTargetHouse` port) answers
  through `FireQuery::can_target_berzerk_house`, whose world implementation
  passes the firer's and target's houses, the house table and the alliance
  graph (`sim/combat/fire_error_world.rs`).
- `sim::combat::fire_error`'s berzerk arm runs after the iron-curtain one:
  a berzerk firer against a techno target outside the admitted houses returns
  `Illegal`, matching the hook's `CannotFire`.

## Native anchors

`TechnoClass_ReceiveDamage_AllowBerzerkOnAllies` (`0x701D2E`, `Ext/Techno/Hooks.ReceiveDamage.cpp:528`)
admits the arm, and the retarget it causes runs in `TechnoClass_SelectAutoTarget_Scan`
(`0x6F93BB`, `Ext/Techno/Hooks.cpp:1574`).

## Residuals

- The hook's `AllowBerzerkOnAllies` companion key is a separate feature.
- The stock `BerserkFriendly`/allied fire gates stay where they were; this
  arm only adds the house filter.
