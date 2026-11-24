# Iron-curtained targeting

Phobos lets a weapon, or the rules, decide whether an iron-curtained target
may be shot, auto-acquired and retaliated against.

```ini
[WEAPON]                            ; WeaponType
CanTarget.IronCurtained=            ; Nullable<bool>, unset inherits the rule
AutoTarget.IronCurtained=           ; Nullable<bool>, unset inherits the rule

[CombatDamage]
CanTarget.IronCurtained=yes         ; Valueable<bool>, human fallback
CanTargetAI.IronCurtained=no        ; Valueable<bool>, AI fallback
AutoTarget.IronCurtained=yes        ; Valueable<bool>, human auto-target fallback
```

Readers: `Ext/WeaponType/Body.cpp:118-119`, declarations `Body.h:50-51`
(constructed unset); `Ext/Rules/Body.cpp:437-439`, defaults `Body.h:831-834`.
The defaults reproduce the stock split: a human may fire at the curtain, a
computer house may not.

## Firing

Hook `0x6FC339` (`Ext/Techno/Hooks.Firing.cpp:313-369`) runs after T21
GetWeapon; the installed jump `0x6FC22A -> 0x6FC24D` removes the stock T16
AI-only check entirely. In the hook, for a techno target, the shot is refused
(`CannotFire`, native `5` = Illegal) when the target is iron-curtained and the
weapon's `CanTarget.IronCurtained`, else the owner's
`CanTarget.IronCurtained` (human) or `CanTargetAI.IronCurtained` (AI), answers
no. Because the arm runs after T17/T19/T20/T21 and the hook's money/cell arms,
an earlier refusal (a cloaked target's Cant, EMP, an empty selected slot)
wins.

VERA keeps this in `sim/combat/fire_error.rs::get_fire_error`: the stock build
keeps its instruction-shaped T16 term, while the Phobos build drops it and
evaluates the override/global chain right after T21, the money arm and
`attach_effect_gate` (the modelled part of the hook's cell arm). `FirerFacts`
carries the two globals and the mode-aware `IsControlledByHuman` answer
(`HouseClass::IsControlledByHuman @ 0x0050B730`); `WeaponFacts` carries the
weapon override.

## Retaliation and auto-target

Hook `0x708AD0` (`Hooks.Firing.cpp:1226-1264`) sits inside
`TechnoClass::ShouldRetaliate` before the Verses test. Only a human owner is
affected: a curtained source is refused (`0x708B17`) when the selected
weapon's `AutoTarget.IronCurtained`, else the global, says no. VERA evaluates
it at the same point in `combat_targeting::should_retaliate`.

Hook `0x6F7CE2` (`Hooks.Targeting.cpp:57-76`) sits in
`TechnoClass::CanAutoTargetObject` right after SelectWeapon, inside the
`mask & 0x18200` guard that also skips the fire-error probe. A human scanner
answers false for a curtained candidate when the weapon override, else the
global, says no; VERA performs it in `greatest_threat::evaluate_candidate`
inside the same mask guard, before the GetFireError probe (G2b).

## Residuals

- The AI firing path is enforced by the CanFire gate itself, so an AI victim
  with `CanTargetAI.IronCurtained=no` also refuses retaliation before the
  human-only ShouldRetaliate gate; that matches native.
- `CanTarget.IronCurtained` governs explicit orders through the same CanFire
  gate; no separate order-time check exists natively.
- The campaign `PlayerControl=` predicate is carried as a distinct fact but
  has no focused regression: the fixtures cannot yet build a house with
  `PlayerControl=yes, IsHuman=no`. Multiplayer is unaffected.
- The mask-guarded auto-target placement has no focused regression; the
  represented scan API takes a mission rather than a raw mask.
- Retail rules author none of these keys; the regressions use synthetic rules
  and the defaults above.
- The C4 planter tests now expect Phobos `GetBurstDelay(0) = 0` (the earlier
  `Burst.Delays` chain), so their stock mid-burst draw expectation is
  feature-aware; that is a test correction, not part of this mechanism.
