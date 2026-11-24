# FireUp.ResetInRetarget

A turretless Unit plays a firing sequence while it shoots. Phobos lets the
target setter clear that sequence on a new target, or keep it when the new
target is already in range.

```ini
[RAIDER]                           ; UnitType art section
FireUp.ResetInRetarget=yes         ; boolean, default yes
```

Reader: `Ext/UnitType/Body.cpp:94`, `Valueable<bool>` constructed true at
`Ext/UnitType/Body.h:67,128`.

## Native body

- `TechnoClass_SetTarget_After` (hook `0x6FCF3E`,
  `Ext/Techno/Hooks.cpp:1164-1184`): for a non-null target on a `UnitClass`
  whose type has no `Turret` and whose `CurrentFiringFrame` is not -1, the
  frame becomes -1 when `FireUp.ResetInRetarget` is true **or** the target is
  not `IsCloseEnough(target, SelectWeapon(target))`.
- The order follows the target store and `UpdateGattlingRateDownReset`; the
  null-target arm (`ResetDelayedFireTimer`) is separate.

## VERA

`ArtEntry::phobos_fire_up_reset_in_retarget` reads the unit's art key.
`Simulation::assign_target_represented` calls `reset_fire_up_in_retarget`
after the gattling reset and null delayed-fire arm; it uses the shared
`keep_target_on_move::is_close_enough_with_weapon` (the `IsCloseEnough`
in-range owner) on the selected weapon slot and resets
`GameEntity::unit_firing_counter` through `UnitFiringCounter::reset`.

## Tests

- `src/sim/mission/fire_up_reset_tests.rs`:
  `the_default_true_key_resets_in_range_retargets`,
  `a_false_key_keeps_only_an_in_range_retarget`,
  `a_turreted_unit_keeps_its_sequence`.

The key arm and the range arm were each mutated red and restored.

## Residuals

- VERA passes the selected weapon slot (-1) instead of resolving
  `SelectWeapon(target)` explicitly; the shared InRange owner selects the same
  weapon for the admitted target.
- The art key is read from the ARTMD layer only, as native does; a unit with
  no art entry keeps the constructor's true.
- The bare `assign_target_stored` representations (for example the Ares
  Temporal bunker assignment in `src/sim/temporal.rs`) call the same native
  setter without this hook, as they already do for the gattling reset. That is
  a pre-existing gap of those representations, not of this arm.
- A store that writes the value already held (a bunker redirect back to the
  current target) would reach the native hook but is skipped here because the
  arm keys on the stored value differing from the previous one.
