# Keep target on move (`KeepTargetOnMove.*`)

A unit can keep shooting at its target while it drives to a move order, as
long as the target stays within reach. Four TechnoType keys
(`Ext/TechnoType/Body.cpp:1103-1106`, fields `Body.h:320-323`, constructor
`Body.h:754-757`):

```ini
[SOMETECHNO]
KeepTargetOnMove=false             ; boolean
KeepTargetOnMove.Weapon=-1         ; integer slot, -1 = SelectWeapon(target)
KeepTargetOnMove.NoMorePursuit=yes ; boolean
KeepTargetOnMove.ExtraDistance=0   ; Leptons (INI cells x 256)
```

## Behavior (native)

- `EventClass_Execute_MegaMission_MoveCommand @ 0x4C7462`
  (`Ext/Techno/Hooks.Misc.cpp:1036-1075`) replaces the target setter for a
  `Mission::Move` order to a Unit with the type key and a live target:
  when the order's own target token is null and the unit's current target is
  close enough to attack (`IsCloseEnoughToAttack @ 0x6F7780`, i.e.
  `SelectWeapon(target)` + InRange with **no** key overrides), the unit keeps the
  target, calls `SetDestination(order destination)` itself, latches
  `UnitExt::KeepTargetOnMove`, and skips the shared target clear and
  destination setter. Every other Unit MegaMission clears the latch and runs
  the stock order.
- `UnitExt::UpdateKeepTargetOnMove` (`Ext/Unit/Body.cpp:94-144`), from the
  `UnitClass_AI @ 0x736480` hook before `DepletedAmmoActions`:
  - no target → clear the latch;
  - type key gone → `SetTarget(null)`, clear the latch;
  - `Guard` mission and `NoMorePursuit=no` → `QueueMission(Attack, false)`,
    clear the latch; `Guard` with `NoMorePursuit=yes` falls through;
  - any other mission except `Move` → return (re-checked later);
  - weapon = `KeepTargetOnMove.Weapon` when >= 0, else
    `SelectWeapon(target)`; with its range temporarily raised by
    `ExtraDistance`, a target no longer `IsCloseEnough` is dropped and the
    latch cleared.

## Where VERA20K owns it

- `rules::phobos_keep_target::PhobosKeepTargetOnMove` (read with
  `read_bool`, `read_ares_int`, `read_phobos_leptons`) on `ObjectType`.
- `GameEntity` carries the per-unit latch (`phobos_keep_target_on_move`,
  serde-defaulted false).
- `sim::keep_target_on_move::move_order_keeps_target` is called from
  `world_commands::apply_megamission_command` at the target-setter point;
  `update_keep_target_on_move` runs in the `UnitClass_AI` hook slot in
  `techno_ai` before the depleted-ammo actions.
- `combat::in_range::in_range_at_slot` is the one InRange body; the Move-order
  arm uses its `-1`/`0` form (`CanFireAtTarget @ 0x6F7780`), and the Unit-AI
  re-check uses an explicit/selected slot with the temporary range extension
  (`IsCloseEnough`, vt+0x3A8). It reuses `compute_range_target` and clones the
  weapon instead of mutating the shared rules record (native writes
  `WeaponTypeClass::Range` around the call; VERA discards the clone before
  `SetTarget(null)`, and no reader observes the shared field in between).

## Residuals (trigger · impact · frequency · risk)

- **Explicit `KeepTargetOnMove.Weapon` slot** uses `combat_weapon::get_weapon`
  for that index; a slot whose record resolves no weapon drops the target.
  Native also rejects a null `GetWeapon(weaponIndex)->WeaponType` by keeping
  the target (its `if (auto pWeapon = ...)` arm). Trigger: an authored
  weapon slot with no weapon. Impact: VERA drops the target one pass earlier.
  Frequency: broken mod data. Risk: low.
- **Map-less fixtures** return `false` from the close-enough check (no cells
  to measure), so the latch is not taken; production always has a map.
- **No retail/visual validation** in this checkout.
