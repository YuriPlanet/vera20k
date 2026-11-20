# Phobos `Anim.Update` firing-anim chain

Keys: `[WeaponType] Anim.Update=` (`Nullable<bool>`, `Ext/WeaponType/Body.cpp:208`),
falling back to `[AudioVisual] FiringAnim.Update=` (`Valueable<bool>`, default
false, `Ext/Rules/Body.cpp:548`). The muzzle type itself comes from the weapon's
`[WeaponType] Anim=` list, an Ares field the stock reader never fills.

## Native evidence

- `0x6FF42B` (`TechnoClass_Fire_Anim`, `Ext/Anim/Hooks.cpp:714-740`): a weapon
  whose `Anim=` list is non-empty and whose `Anim.Update` resolves true hands
  the flash its weapon, weapon index, burst index and the firing count starts
  (`ScenarioExt::Global()->FiringAnimUpdateCount++`). The hook then skips the
  building check at `0x6FF437`.
- `Phobos AnimExt` (`Ext/Anim/Body.h`): `FiringAnim_Weapon`,
  `FiringAnim_WeaponIndex`, `FiringAnim_BurstIndex`, `FiringAnim_LastFacing`,
  `FiringAnim_LastCoords`.
- `0x423B95` (`AnimClass_AI_Early`, `Ext/Anim/Hooks.cpp:714-740`): while
  `FiringAnimUpdateCount > 0`, every anim runs `UpdateAsFiringAnim`
  (`Ext/Anim/Body.cpp:72-105`):
  - `GetRealFacing()` (`0x708D70` forwards to the turret-facing virtual
    `vt+0x2A8`); a changed facing re-picks the type from the weapon's `Anim=`
    list with `GeneralUtils::GetItemForDirection`
    (`Utilities/GeneralUtils.h:66-93`) and assigns `Anim->Type`.
  - `GetRenderCoords()` (`0x41BE00` copies `GetCoords`) or a facing change
    re-anchors: `Anim->SetLocation(GetFLH(weaponIndex) - GetRenderCoords)`,
    restoring the recorded burst index only around the `GetFLH` call.
- `~AnimExt` decrements `FiringAnimUpdateCount` for every marked anim.

## Implementation

- `sim::anim_class` owns `FiringAnimState` on the anim, `attach_firing_anim`
  (the `0x6FF42B` trigger, called from the muzzle-anim admission), the
  per-visit pass `update_as_firing_anim` (called at the top of `visit_anim`
  while the count is non-zero) and `get_item_for_direction`. The trigger
  stores only the weapon, weapon index and burst; `FiringAnim_LastFacing` and
  `FiringAnim_LastCoords` start zero (`Ext/Anim/Body.h:57-61`), so the first
  pass always re-picks and re-anchors, as native does.
- The pass reads the owner's `FireSource`, resolves the recorded weapon and
  direction index, retypes through `retype_for_firing_anim` (frame counters
  carry over, only the type bounds are re-derived), and writes the stored
  location as `fire.coord - GetCoords`, which is owner-relative for an
  attached anim.
- The count is released in both physical removal funnels
  (`scalar_delete_anim` and the deferred `remove_live_object` Anim arm).

## Residuals

- `GetRealFacing` is modelled as `fire_coordinate`'s aim facing: the barrel
  facing when the object has one (every aircraft carries one for its Fly
  secondary heading), the body heading otherwise. That matches `0x708D70`'s
  delegation to `TurretFacing` for turreted objects; whether an aircraft's
  `TurretFacing` returns its secondary heading or the body heading is not
  proven here.
- A building's re-anchor takes `fire_coordinate`'s `TarCom`-driven fire facing
  rather than a live turret facing; stock buildings with a turret re-aim at the
  same target, so the values agree in play.
- The `0x6FF437` skip after the marker is not modelled: the building muzzle
  `ZAdjust` keeps its current value for a marked anim. The instruction range
  the hook skips has not been decoded to a behaviour.
- The pass runs inside the anim's own AI visit (`visit_anim`) instead of a
  separate early-AI gate; the same-frame order relative to the stock anim body
  is preserved.
- No RNG draws; the count is transient state saved with the scenario like
  native `ScenarioExt`.
