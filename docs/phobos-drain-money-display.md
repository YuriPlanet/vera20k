# Drain money display

A DrainWeapon link moves money from the drained building to the drainer every
`DrainMoneyFrameDelay` frames. Phobos replaces that native block's code with
its own (`TechnoClass_AI_DrainMoney`, hook `0x6FA167`, skip to `0x6FA1C5`;
`Ext/Techno/Hooks.cpp:172-223`) and adds flying credit labels around the same
transfer. The labels are client-side text: they never change the wallet.

```ini
[DISK]                               ; TechnoType
DrainMoneyDisplay=yes                ; boolean, unset = [AudioVisual] value (no)
DrainMoneyDisplay.Houses=enemies     ; AffectedHouse, unset = [AudioVisual] value (all)
DrainMoneyDisplay.Offset=0,0         ; screen-pixel offset, default 0,0
DrainMoneyDisplay.OnTarget=yes       ; boolean, unset = [AudioVisual] value (no)
DrainMoneyDisplay.OnTarget.UseDisplayIncome=no  ; boolean, unset = [AudioVisual] value (yes)

[NAREFN]                             ; BuildingType, read only by the last arm
DisplayIncome.Houses=all             ; AffectedHouse, unset = [AudioVisual] value (all)
DisplayIncome.Offset=0,0             ; screen-pixel offset, default 0,0
```

The bracketed fallbacks are the `[AudioVisual]` globals Phobos' own rules pass
reads (`Ext/Rules/Body.cpp:291-304`): `DrainMoneyDisplay`,
`DrainMoneyDisplay.Houses`, `DrainMoneyDisplay.OnTarget`,
`DrainMoneyDisplay.OnTarget.UseDisplayIncome` and `DisplayIncome.Houses`. All
type keys except the offsets are `Nullable`, so `<default>`/`none` clears them
and an absent key keeps the current value.

## Behavior

On a drain frame whose clamped amount is non-zero, after the two wallet writes:

1. With `DrainMoneyDisplay` (type or global) the drainer gets a `+amount`
   label at its own `Location`, filtered by `DrainMoneyDisplay.Houses` and
   offset by `DrainMoneyDisplay.Offset`.
2. With `DrainMoneyDisplay.OnTarget` the drained object gets a `-amount` label
   at its `GetRenderCoords` (`0x0041BE00`, the building foundation's centre).
   - `UseDisplayIncome=no` keeps the drainer's House as the label's owner for
     the House filter (`EnumFunctions::CanTargetHouse`), the drain Houses
     filter and the drain offset.
   - `UseDisplayIncome=yes` on a **building** target uses the building's own
     House, its type's `DisplayIncome.Houses` (falling back to the global) and
     `DisplayIncome.Offset`. The `DisplayIncome` switch and its accumulated
     income are not consulted.

The label's shroud test uses the anchor point; the House filter and visibility
run per client against the local player, exactly as `FlyingStrings::
AddMoneyString` does for DisplaySoylent (`Ext/Misc/FlyingStrings.cpp:28-47`).

## Where VERA20K owns it

Rules live in `rules::phobos_drain_money_display` (the type keys plus the
consumed `DisplayIncome.Houses`/`.Offset`) and in `rules::phobos_globals` (the
`[AudioVisual]` fallbacks, read once from the merged layer state).
`sim::credit_income::drain_common_step` emits the labels through
`FrameEffects`'s money-text boundary; the client's
`app::presentation::flying_credits` owns the shroud, House, visibility and
pixel-offset checks. `flying_text::BountyText::display_owner` carries the
drainer's House for the target label whose anchor is the building.

## Residuals

- The drainer's `IsClearlyVisibleTo(HouseClass::CurrentPlayer)` gate is not a
  separate query here; the client's existing `VisualCharacter == Hidden`
  rejection stands in, the same approximation the DisplaySoylent label uses. A
  partially cloaked drainer can therefore show a label native would hide.
- `TechnoType`'s `DrainMoneyFrameDelay`/`DrainMoneyAmount`
  (`Ext/TechnoType/Body.cpp:1162-1163`) remain globally sourced; the label
  always follows the transfer this port actually makes.
- The `DisplayIncome` feature itself (accumulated income and its periodic
  label) is a separate chain; only the two reads the drain label consumes are
  ported.
