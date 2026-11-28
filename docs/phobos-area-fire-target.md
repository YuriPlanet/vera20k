# AreaFire.Target

An `AreaFire=yes` weapon normally fires at the cell under its firer. Phobos
lets the weapon aim elsewhere.

```ini
[NukeEruption]                  ; [WeaponTypes] section
AreaFire.Target=base            ; base | random | self (default base)
```

Reader: `Ext/WeaponType/Body.cpp:123` (`Valueable<AreaFireTarget>` declared
`Ext/WeaponType/Body.h:55`, constructed `Base` `:150`; enum
`Utilities/Enum.h:171-177`; its `read` at `Utilities/TemplateDef.h:952-977`
keeps the current value for an unknown token).

## Native body

`TechnoClass_FireAt_AreaFire` (`0x6FE19A`,
`Ext/Techno/Hooks.Firing.cpp:743-798`) replaces the stock target before the
shot:

- `random`: the range in cells is `GetRangeWithModifiers(weapon, firer)` / 256
  rounded down through `+ 0.99`; `AdjacentCellsInRange` enumerates the cells in
  `CellSpreadEnumerator` order. Each try spends one
  `RandomRanged(0, last)` Scenario draw, takes `(i + rand) % size`, adds that
  offset to the aimed cell and admits it when the cell exists, the weapon's
  `SkipWeaponPicking` derivation is true, or
  `AreCellAndObjectsEligible(cell, CanTarget, CanTargetHouses, owner, …)`
  passes. Running out of candidates answers `0x6FE4E7` — `xor eax,eax; ret 8`,
  no shot and no rearm.
- `self`: when the gate refuses the firer's own cell the shot does not fire;
  otherwise the hook stores the firer object as the cell argument
  (`R->EAX(pThis)` at `Ext/Techno/Hooks.Firing.cpp:789`) and jumps to
  `0x6FE1D5`, past the block's `SetTarget`. The shot aims at the firer; its
  target coordinate is the object's own `GetCoords` (VERA reads the object
  centre, [`target_coords`]).
- `base`: the stock cell under the firer, gated the same way. Both `base` and
  `self` gate with `owner = nullptr` and `explicitEmptyCells = false`; the
  `random` arm passes the firer's owner and `true`.

The gate's `SkipWeaponPicking` is derived at load: true exactly when no
`CanTarget`, `CanTargetHouses`, health-range, veterancy or AE list is authored
(`Ext/WeaponType/Body.cpp:210-223`).

## VERA

`PhobosWeaponExtras::area_fire_target` reads the key;
`combat::world_fire` resolves it at the stock area-fire cell block. `random`
reuses `cell_rect::cell_spread_offsets` (the `CellSpreadEnumerator` port) and
the Scenario stream; `self` swaps the snapshot to `TargetKind::Entity(firer)`
and the firer's own coordinates; `base` keeps the previous behaviour. The gate
ports `AreCellAndObjectsEligible` over `crit::cell_eligible`,
`crit::techno_eligible`, `combat::can_target_house` and the hook's
`allowBridges` formula (candidate bridge head and the firer's
bridge/level). A `random` shot with no admitted candidate returns the no-shot
path before any bullet work; `base`/`self` return it when the firer's cell is
refused.

## Tests

- `src/rules/phobos_weapon_extras_tests.rs` — the three names, case folding
  and the unknown-name fallback.
- `src/sim/combat/combat_tests.rs` —
  `phobos_area_fire_target_self_and_random_replace_the_stock_cell`: a deployed
  Desolator's area-fire weapon keeps `Cell(10,10)` for `base`, targets the
  firer for `self`, for `random` lands on
  `cell + offsets[RandomRanged(0, last)]` predicted from a Scenario-stream
  clone, and does not fire at all when `CanTarget=units` refuses the infantry
  firer's own cell. Mutating the mode gate to `base` turns the self row red;
  restored.

## Residuals

- The random arm's range input reuses the shared attached-effect range fold;
  the native `GetRangeWithModifiers` also substitutes `[CombatDamage]
  OccupyWeaponRange` for a firer that `CanOccupyFire` (occupied buildings) and
  applies the Ares veteran `Range` multiplier. Neither is modelled here, so
  the candidate list can differ for those firers.
- A negative `Range=` (the `-512` sentinel) answers one candidate in VERA; the
  native `static_cast<size_t>` wraps into `CellSpreadEnumerator::Max`, which no
  retail area-fire weapon reaches.
- Native's `skipWeaponPicking ||` short-circuits the cell-existence test, so an
  all-gate-free weapon can take an off-map candidate and fire at a null cell.
  VERA skips off-map candidates in every case.
- The `self` arm's target coordinate is inferred as the object centre; later
  hook lines only show `R->EAX(pThis)` and the `0x6FE1D5` jump.
- `AdjacentCellsInRange`'s `CellSpreadEnumerator` port caps the spread radius
  at 256 (the native `Max`), i.e. a 257x257 candidate square at the cap.
