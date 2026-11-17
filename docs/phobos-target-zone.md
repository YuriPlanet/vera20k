# Optional Phobos target zone scan

This key decides whether a unit looking for something to attack may pick a
target it cannot drive to: across water, up a cliff or in a closed-off pocket.
It works in every build: the key in the rules, mode or map turns Phobos on for
the game ([the Phobos switch](phobos.md#the-phobos-switch)).

```ini
[SOMETECHNO]             ; TechnoType
TargetZoneScanType=same  ; same, any or inrange
```

A hunting unit, or a team member whose script attacks a kind of target
(script actions 0 and 57), looks over every object on the map and skips those
outside its own movement zone, the area its `MovementZone` lets it reach from
where it stands.

- `same` (the default): it skips them, as in the original.
- `any`: it may pick them.
- `inrange`: it may pick one when the weapon it would use against it reaches
  it from the passable cell nearest it, found as for a unit of its
  `SpeedType` and `MovementZone`.

Aircraft and buildings have no movement zone and are never limited, and the
ring scans of guarding units are not changed. Once Phobos plays, `same` looks
a building target's zone up at its top-left cell rather than its centre, as
Phobos does.

Source: `TechnoTypeExt` reads the key (`src/Ext/TechnoType/Body.cpp`), and
`TechnoExt::AllowedTargetByZone` (`src/Ext/Techno/Body.cpp`) makes the test
from the hooks `TechnoClass_GreatestThreat_MapZoneSetContext` (0x6F9C67) and
`TechnoClass_EvaluateObject_MapZone` (0x6F7E47) in
`src/Ext/Techno/Hooks.TargetEvaluation.cpp`, which replace the original's
test at 0x6F7E47..0x6F7EA1 in `TechnoClass::Evaluate_Candidate`.

## Where VERA20K owns it

- `rules/object_type.rs` — the per-type read (`TargetZoneScanType`).
- `rules/phobos_switch.rs` — whether Phobos plays (`RuleSet::phobos`).
- `sim/combat/greatest_threat.rs` — the zone test (`zone_admits`) in the
  scan over every object.

## Residuals

- Phobos' own attack script actions, which make the same test, are not
  ported.
- With `inrange`, a weapon slot that holds no weapon (a type without one)
  makes the unit skip the target; Phobos would read the missing weapon.
