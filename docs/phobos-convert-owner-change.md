# Convert.HumanToComputer / Convert.ComputerToHuman

A Foot can convert to another type when its owner changes between a human and
a computer house, and a computer-to-human change also repairs its mission.

```ini
[FOOT]                                ; TechnoType
Convert.HumanToComputer=              ; Valueable<TechnoTypeClass*>, absent/none = off
Convert.ComputerToHuman=              ; Valueable<TechnoTypeClass*>, absent/none = off
```

Readers `Ext/TechnoType/Body.cpp:1015-1016`, declarations
`Ext/TechnoType/Body.h:250-251`, constructor null `:682-683`.

## Behavior

`TechnoClass_Captured_UpdateTracking @ 0x7015C9`
(`Ext/House/Hooks.cpp:203-300`). The hook captures the old owner's
`IsControlledByHuman` and the new owner's before the swap; with a Foot and a
human<->computer difference it converts through the OLD type's key
(`Convert.HumanToComputer` when the old owner was human, otherwise
`Convert.ComputerToHuman`), then, when the old owner was computer-controlled,
runs `TechnoExt::ChangeOwnerMissionFix`
(`Ext/Techno/Body.Internal.cpp:234-260`) using the old type:

- clear `ShouldEnterAbsorber` and `ShouldEnterOccupiable` (not the
  nearest-occupiable seek);
- return if the Foot still has any radio link or the old type is
  `ResourceGatherer=`;
- return on Harvest, Sleep, Harmless or Repair;
- otherwise `Override_Mission(Guard, nullptr, nullptr)`, set
  `ShouldLoseTargetNow`, then `Queue_Mission(DefaultToGuardArea ? AreaGuard :
  Guard, 1)`.

Both houses request a tech-tree recheck in the Foot arm.

## Where VERA20K owns it

`Simulation::phobos_convert_on_owner_change`
(`sim/game_entity/type_conversion.rs`) is called from the shared
`ChangeOwner` path with the old owner still installed, beside the AutoDeath
owner-change hook, before the swap. It converts through
`convert_techno_type` and runs the mission fix through the existing
`mission_override_represented`, the `passive_scan` lose-target latch and
`mission_queue_exact(..., immediate = 1)`. The seeks are cleared with a new
`BuildingEntryLatches::clear_mission_fix_seeks` that leaves `+0x691` set.

RESIDUAL: native also clears `ShouldScanForTarget` and
`ShouldGarrisonStructure`; VERA has no owner for either byte, so a converted
object can keep a pending scan/garrison latch.

RESIDUAL: `HasAnyLink()` is modelled as a non-empty `radio_contacts` list.
Native's slot test may differ for a stale slot; trigger: converting a Foot
with a stale radio slot whose mission is otherwise repairable.

RESIDUAL: the conversion body is the Ares-shaped `ConvertToType` in both
builds (see `docs/phobos-convert-health.md`).
