# Optional Phobos airstrike target eligibility

Build with `--features phobos` to choose which types an `Airstrike=` warhead
may designate, independently of `CanC4=`, and to let the aircraft follow a
non-building target instead of the cell it stood on.

```ini
[SOMETECHNO]                  ; a TechnoType
AllowAirstrike=               ; boolean, unset keeps the vanilla rule

[SOMEWARHEAD]                 ; a WarheadType
AirstrikeTargets=buildings    ; none|infantry|units|buildings|aircraft|all
```

`AirstrikeTargets` is a comma list, read case-insensitively and trimmed. A
token that names no target leaves the whole key at its default, `buildings`.
`aircraft` reads the `units` bit: Phobos' eligibility test does not consider
aircraft separately.

`AllowAirstrike` unset keeps the vanilla rule at each site: a building follows
`CanC4`, every other kind is a valid target. The cursor keeps its
`InvisibleInGame` refusal for buildings.

The weapon-choice arm applies one more vanilla default: a building that is
both a resource destination and a resource gatherer is refused unless
`AllowAirstrike` says otherwise. The cursor does not apply that clause, as in
Phobos.

## Native sites

- `TechnoClass::What_Weapon_Should_I_Use` arm I at `0x006F3477` returns the
  secondary only for a C4-able building that is not both a resource
  destination and a resource gatherer. Phobos
  `TechnoClass_WhatWeaponShouldIUse_Airstrike` (`0x006F348F`) returns the
  primary unless the target kind is in `AirstrikeTargets` and the target type
  permits the strike.
- `TechnoClass::What_Action` `0x0051EAE0` tests the hovered building's
  `CanC4`. Phobos `TechnoClass_WhatAction_AllowAirstrike` reads
  `AllowAirstrike` for every kind.
- `TechnoClass::CanFire` refuses a shot at a kind the warhead excludes or a
  type that refuses the strike
  (`src/Ext/Techno/Hooks.Firing.cpp`), whatever the weapon slot.
- `AirstrikeClass::Fire @ 0x0041D97B` continues into the code that converts
  the target to its cell only for a building
  (`src/Ext/Techno/Hooks.Airstrike.cpp`). Every other target stays the object,
  so the sortie's aircraft chase a moving unit.
- `TechnoClass::DrawAirstrikeFlare` draws its line only when the manager's
  target is a building. Phobos `DEFINE_JUMP(LJMP, 0x6D481D, 0x6D482D)`
  ("Allow airstrike flare draw to foot") skips that gate, so the line reaches
  a foot target as well.

## Where VERA20K owns it

- `rules/airstrike_targets.rs` — the bit set, its name list and the
  aircraft-reads-units rule.
- `rules/warhead_type.rs` — the `AirstrikeTargets=` read.
- `rules/object_type.rs` — the `AllowAirstrike=` read and its two resolution
  helpers (cursor and weapon choice).
- `sim/combat/combat_weapon.rs` — arm I of the weapon choice.
- `sim/combat/fire_error.rs` — the `CanFire` gate.
- `sim/world/airstrike_aircraft.rs` — the object-versus-cell target.
- `sim/airstrike.rs` — the designation line's building gate.
- `app/input/context_order.rs` — the cursor answer.

The strike itself already addressed a target id; only the aircraft's own
attack target had to stop being a cell, and the designation line had to stop
requiring a building.
