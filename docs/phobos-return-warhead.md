# ReturnWarhead

A warhead can strike the techno that fired it back.

```ini
[WH]                                  ; Warhead
ReturnWarhead=                        ; WarheadType to send back
ReturnWarhead.Damage=0                ; Valueable<int>
ReturnWarhead.Chance=1.0              ; Valueable<double> (percent or absolute)
ReturnWarhead.FullDetonation=true     ; Valueable<bool>
ReturnWarhead.AffectsTarget=all       ; AffectedTarget
ReturnWarhead.AffectsHouse=all        ; AffectedHouse
ReturnWarhead.ApplyChancePerTarget=   ; Nullable<bool>, global [CombatDamage]
```

Reader `Ext/WarheadType/Body.cpp:235-241`; defaults `Body.h:360-366`.

## Behavior

`WarheadTypeExt::ApplyReturnWarhead` (`Ext/WarheadType/Detonate.cpp:610-636`),
called once per target in `DetonateOnOneUnit` after the displacement arms and
before `PenetratesTransport`:

- the dice is a fresh Scenario draw when the type or global
  `ReturnWarhead.ApplyChancePerTarget` is set or the type-level
  `ApplyPerTargetEffectsOnDetonate` is off, otherwise the per-detonation
  `ReturnWarhead_RandomBuffer`; `ReturnWarhead.Chance < dice` refuses;
- with a known source house, `AffectsHouse` must accept the victim's owner;
- the victim's cell and class must pass `AffectsTarget` (`IsCellEligible`
  with `explicitEmptyCells=false`, `IsTechnoEligible`);
- `FullDetonation` (default true) runs
  `DetonateAt(ReturnWarhead, firer coords, victim as attacker, Damage,
  victim house)`; otherwise the firer takes a direct `ReceiveDamage` with
  that warhead, the victim as attacker and the victim's house.

The return needs the bullet's firer: a `source_id == 0` detonation has none
and does nothing.

## Native anchors

`ReturnWarhead` runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`); the returned detonation re-enters
that same body through the stock bullet path.

## Where VERA20K owns it

`rules::phobos_return_warhead` reads the keys onto `WarheadType`;
`crit::detonate` counts a return warhead in its per-target gate and stores
the once-per-detonation buffer in `CritRoll`;
`sim::combat::phobos_return_warhead::apply` runs at the `DetonateOnOneUnit`
position with the shared `cell_eligible`/`techno_eligible` helpers, commits
the full arm as a `ProjectileDetonationReason::ReturnWarhead` packet and the
direct arm as an `EntityDamageEvent::direct_receiver` through
`commit_entities`.

`ReturnWarhead=` resolves through `Valueable<WarheadTypeClass*>` `Find` at
read time, so an unknown name stays null and the warhead never returns; VERA's
apply gate requires a resolvable warhead, so a misspelled name likewise
neither returns nor draws the return pass. The reader only changes at which
point the name is resolved; no observable residual.

RESIDUAL: the full arm needs the original firer entity; when it is already
gone from the substrate the return does nothing, where native still holds
the Techno pointer for the current frame.
