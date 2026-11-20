# Warhead `Taunt`

`[WarheadType] Taunt=yes` forces every target hit by the warhead onto
`Mission::Attack` against the techno that fired it. Plain `Valueable<bool>`
read by `Ext/WarheadType/Body.cpp:444`, default `no`.

```ini
[WARHEAD]
Taunt=yes
```

## Native evidence

`Ext/WarheadType/Detonate.cpp:236-237`, inside `DetonateOnOneUnit` after the
disguise/reverse-engineer/return-warhead/penetrate arms and before the shield
and AttachEffect blocks:

```
if (this->Taunt && pOwner)
    pTarget->Override_Mission(Mission::Attack, pOwner, nullptr);
```

`pOwner` is the firing techno (the bullet's owner), not merely its house; with
no owner nothing happens. The override runs once per affected target, so a
multi-target warhead taunts each one at that owner.

## Where VERA20K owns it

- `rules::warhead_type::WarheadType::taunt`.
- `sim::combat::crit::detonate`'s per-target loop, after
  `penetrates_transport::apply` and before the shield modifiers
  (`sim/combat/crit.rs:339`), calls
  `Simulation::override_mission_on_damage_response(target, source, ...)` — the
  concrete `Override(Attack, attacker, NULL)` transaction
  (`sim/mission/authority.rs:663`) with the same mission/target archive.
- `source_id == 0` (no firing techno) skips the arm.

## Native anchors

`Taunt` runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`), on the same one-time effects that
`DetonateOnOneUnit` applies.

## Residuals

- The override is the same transaction the retaliation path uses; its class
  setters decide target admission, matching native's dispatch order.
- Retail/visual parity is untested here (no retail `ini/` in this checkout).
