# Warhead `RemoveDisguise`

`[WarheadType] RemoveDisguise=yes` strips a disguised target's disguise when
the warhead reaches it. It is a plain `Valueable<bool>` read by
`Ext/WarheadType/Body.cpp:177`, default `no`.

```ini
[WARHEAD]
RemoveDisguise=yes
```

## Native evidence

`Ext/WarheadType/Detonate.cpp:224-225` calls `ApplyRemoveDisguise(pTarget)`
inside `DetonateOnOneUnit`'s one-time effects, after the `Ammo` arm and
before `ReverseEngineer`/`ReturnWarhead`. The body (`:466-475`):

```
if (pTarget->IsDisguised())
{
    if (pSpy = specific_cast<InfantryClass*, true>(pTarget))
        pSpy->Disguised = false;          // flag only
    else if (pMirage = specific_cast<UnitClass*, true>(pTarget))
        pMirage->ClearDisguise();         // flag, type and house
}
```

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::remove_disguise`.
- `sim::combat::crit::detonate`'s per-target loop applies it before the
  return-warhead arm (`sim/combat/crit.rs:265`), using
  `GameEntity::disguise` / `DisguiseRuntime`:
  - `clear_flag` for a non-Unit (infantry), keeping the recorded type/house.
  - `clear_unit` for a Unit, clearing the identity too.
- The disguise runtime is only created when it already existed; a
  non-disguised target is untouched.

## Native anchors

`RemoveDisguise` runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`); the disguise it clears is the
stock `UnitClass` disguise state.

## Residuals

- The damage receiver's own `receive_damage_reveal` also clears a disguise on
  damage; the warhead arm is the separate zero-damage path, matching native's
  one-time effect.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
