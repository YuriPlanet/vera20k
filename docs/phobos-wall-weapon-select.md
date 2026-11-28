# AllowWeaponSelectAgainstWalls

A wall overlay is judged by the weapon that can actually hurt it. Phobos lets
a type fall back to its secondary wall-breaker when the current weapon cannot.

```ini
[CombatDamage]
AllowWeaponSelectAgainstWalls=no   ; Valueable<bool>, default no

[DUAL]                              ; TechnoType section
AllowWeaponSelectAgainstWalls=yes   ; Nullable<bool>, unset -> the global
```

Readers: `[CombatDamage]` at `Ext/Rules/Body.cpp:256` (constructor `false`,
`Ext/Rules/Body.h:174,702`); the type key at `Ext/TechnoType/Body.cpp:909`
(`Nullable<bool>`, `Body.h:169,565`), whose unset state passes the global
through.

## Native body

`TechnoExt::GetWeaponIndexAgainstWall` (`Ext/Techno/WeaponHelpers.cpp:196-228`)
replaces the hardcoded `GetWeapon(0)` at three hooks:

- `0x51C1F1` — `InfantryClass::Can_Enter_Cell`'s wall arm;
- `0x73F495` — `UnitClass::Can_Enter_Cell`'s wall arm (native
  `0x73F495` reads slot `0` and tests `Wall=` at warhead `+0x144`, `Wood=` at
  `+0x147`);
- `0x70095A` — `TechnoClass::What_Action`'s wall rung (the ATTACK cursor).

The helper starts from `GetCurrentWeapon`'s slot (`0x70E1A0`: the gunner slot
for a `TurretCount>0` non-gattling type, the gattling stage pair, else slot
0) and returns it when the type is a turreted non-gattling one, the overlay is
not a wall, or the key resolves false. Otherwise, if that weapon cannot hurt
the wall — no `Wall=` and no `Wood=` against an `Armor=wood` overlay — or
`TechnoExt::CanFireNoAmmoWeapon(pThis, 1)` holds (out of ammo and slot 1 is
the no-ammo or `-1` slot), or `AttachEffect.CheckOnFirer` refuses its firer,
the secondary slot (`stage*2+1` for a gattling type, else 1) answers when its
warhead can hurt the wall and `NoSecondaryWeaponFallback` allows it.

## VERA

`combat::combat_weapon::weapon_index_against_wall` owns the slot choice.
`Simulation::foot_can_enter`'s wall arm and
`Simulation::cell_attack_rung_admits`'s wall rung read the returned slot's
weapon for their `Wall=`/`Wood=` tests; without the feature they keep the
precomputed primary. The AE gate reuses
`Simulation::weapon_attach_effects_allow` with the firer as its own target.
`SkipWeaponPicking` is not consulted: Phobos derives it false whenever an AE
required list exists, and the gate can only refuse a firer when one does.

## Tests

- `src/sim/world/phobos_wall_weapon_tests.rs` — through the production entry
  receiver and What_Action rung: the global key alone admits the dual-weapon
  unit against an enemy wall, the type key overrides it both ways,
  `NoSecondaryWeaponFallback` keeps the refusal, and a wall-breaking primary
  admits in every row. The helper gate was mutated red (always the current
  slot) and restored.

## Residuals

- Phobos `0x6F8C9D` (`Ext/Techno/Hooks.TargetEvaluation.cpp:120-140`) makes
  the cell-threat fallback use the `SelectWeapon(cell)` slot for its internal
  `GetWeapon`/`GetWeaponRange` calls instead of slot 0. That is a keyless
  Phobos fix on the AI wall scan, not part of this key; VERA's
  `wall_threat_score` still reads the current weapon and GetWeaponRange(0).
- The `SkipWeaponPicking` optimization itself is not read; see the note
  above for why this consumer does not need it.
- `NoAmmoAmount=`/`NoAmmoWeapon=` live in VERA's Ares-gated
  `techno_combat` data, so a game not read the Ares way
  uses the constructor defaults (`0`, `-1`) for the
  `CanFireNoAmmoWeapon(1)` term even when a section authors the keys.
- The test drives the unit wall arm (`0x73F495`) and the ATTACK rung
  (`0x70095A`). The infantry arm (`0x51C1F1`, same helper), the gattling
  slot pair, turreted early return, the AE gate and the ammo term are
  ported from the native arms but not covered by a regression test.
