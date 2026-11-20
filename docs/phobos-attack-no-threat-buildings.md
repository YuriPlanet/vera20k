# Non-threatening building auto-target (`AttackNoThreatBuildings`)

Phobos lets a weapon's own key decide whether its firer may auto-acquire an
enemy building that is unarmed or poses no threat. The stock engine refuses
that building for a human player; the computer's units shoot it, because
`pThis->Owner->IsControlledByHuman()` selects which `[General]` value applies.

## Keys

| Key | Owner | Type | Default |
|---|---|---|---|
| `AttackNoThreatBuildings=` | WeaponType | `Nullable<bool>` | unset → the `[General]` pair below |
| `AutoTarget.NoThreatBuildings=` | `[General]` | `Valueable<bool>` | `no` (human scanners) |
| `AutoTargetAI.NoThreatBuildings=` | `[General]` | `Valueable<bool>` | `yes` (computer scanners) |

Reads: `WeaponTypeExt::LoadFromINIFile` (`Ext/WeaponType/Body.cpp:206`, field
`Ext/WeaponType/Body.h:114`, constructor `:210`); globals
`RulesExt::LoadFromINIFile` (`Ext/Rules/Body.cpp:517-518`, constructors
`Ext/Rules/Body.h:931-932`). VERA reads them in
`src/rules/phobos_weapon_extras.rs` and `src/rules/phobos_globals.rs`.

## Behaviour

`TechnoClass_CanAutoTarget_AttackNoThreatBuildings` (`0x6F85CF`,
`Ext/Techno/Hooks.cpp:1848-1858`) runs inside `TechnoClass::CanAutoTarget`'s
building gate. It resolves

```
canAttack = human ? AutoTarget_NoThreatBuildings : AutoTargetAI_NoThreatBuildings
canAttack = weaponExt->AttackNoThreatBuildings.Get(canAttack)
```

and, when true, jumps straight to the accept tail at `0x006F8604`. The hook
sits at `0x6F85CF`, before the gate's weapon test at `0x006F85D3`, so it also
admits a building that holds no weapon at all — the seven retail buildings
that author a non-zero `ThreatPosed=` without any weapon key (Chronosphere,
Weather Control, Iron Curtain, Genetic Mutator, the dummy, `YAPPET`,
`AMMOCRAT`).

VERA applies it at the same gate in `src/sim/combat/greatest_threat.rs`
("G24"), where the human-attacker building reject lives. The resolved weapon is
the scan's selected slot, matching the hook's `CanAutoTargetTemp::WeaponExt`.

## Residuals

- The gate is reached only for a human scanner without a team and not on an
  aggressive attack-move, which is the same condition the stock reject already
  has; the AI half of the `[General]` pair is therefore read but dormant
  through this consumer. A future `CanAutoTarget` caller for computer scanners
  would use it.

## Tests

- `src/rules/phobos_weapon_extras_tests.rs` — the three keys and both global
  fallbacks.
- `src/sim/combat/greatest_threat_attack_no_threat_tests.rs` — an unarmed
  `ThreatPosed=30` building is refused by default, admitted by the weapon's
  `yes` and by `[General] AutoTarget.NoThreatBuildings=yes`, and refused again
  by an explicit weapon `no`.
