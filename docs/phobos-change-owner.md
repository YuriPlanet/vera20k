# ChangeOwner warhead keys

A warhead transfers the target to the firing House.

```ini
[Warhead]
ChangeOwner=no                     ; Valueable<bool>
ChangeOwner.SetAsMindControl=no    ; Valueable<bool>
ChangeOwner.MindControlAnim=       ; Nullable<AnimTypeClass*>
```

Reader `Ext/WarheadType/Body.cpp:431-433`, declarations
`Ext/WarheadType/Body.h:252-254`; consumer
`WarheadTypeExt::ExtData::ApplyOwnerChange`
(`Ext/WarheadType/Detonate.cpp:207-211,488-522`), reached from
`DetonateOnOneUnit` after `RemoveMindControl` and before `ApplyConvert`.

## Behavior

- `ChangeOwner`: `SetOwningHouse(firing House, true)` on every target the
  detonation visits. Both modes refuse an already mind-controlled target;
  only `SetAsMindControl` additionally refuses `ImmuneToPsionics=`.
- `ChangeOwner.SetAsMindControl=yes` additionally writes native
  `MindControlledByAUnit` (Techno `+0x2C4`, VERA's
  `MindControlLink::permanent`), so the target counts as mind-controlled and
  a second such hit refuses; when `ChangeOwner.MindControlAnim` resolves, it
  rings the target: a unit at `MindControlRingOffset=`, a building at its
  `Height=` in levels, with `ZAdjust = -1024` and the target as the ring's
  owner-object. VERA reuses the capture manager's `CreateMindControlRingAnim`
  owner for this.

## Native anchors

The warhead arm runs inside the warhead body Phobos hooks at `BulletClass_Detonate` (`0x46920B`, `Ext/WarheadType/Hooks.cpp:8`) (`Ext/WarheadType/Detonate.cpp`); the ownership write itself is the
stock `BuildingClass::ChangeOwner` (`0x004483C6..0x0044848F`), already owned elsewhere in VERA.

## Where VERA20K owns it

`WarheadType::phobos_change_owner*` feed
`sim::combat::crit::apply_change_owner`; the ring is
`sim::capture_manager::attach_control_ring` and the house transfer is
`Simulation::change_owner_with_rules`.

The ring anchors at the target's `Location` (`pTarget->Location`,
`Ext/WarheadType/Detonate.cpp:504`), so a building uses its top-left cell
rather than the foundation centre; `a_building_ring_anchors_at_its_location`
covers the offset through `attach_control_ring`. No residual.
