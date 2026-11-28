# Gattling rate-down customization

Phobos reshapes how a gattling type's value falls when it stops firing
(`Ext/TechnoType/Body.cpp:1049-1052`, hooks in `Ext/Techno/Hooks.Misc.cpp:648-745`
and `Ext/Techno/Body.Update.cpp:568`).

```ini
[GTNK]                         ; TechnoType
RateDown.Delay=0               ; game frames before the decay starts; negative disables it
RateDown.Reset=false           ; zero the value when the target changes or is lost
RateDown.Cover.Value=0         ; decay amount while ammo is at or below the threshold
RateDown.Cover.AmmoBelow=-2    ; ammo threshold for the cover value
```

## Behavior

A spin-up restarts the delay. Each later decay call accumulates its ticks; while
the accumulator has not passed `RateDown.Delay`, the value is left alone, and
once it passes, the decay subtracts `RateDown * (ticks past the delay)` so the
held frames are not repaid. A negative delay disables the value decay
completely. At or below `RateDown.Cover.AmmoBelow` ammo, `RateDown.Cover.Value`
replaces the type's `RateDown`. A zero decay amount zeroes the value, as in the
native body. Both native decay paths are covered: the building's idle
`DecreaseValue` and the `UpdateGattlingStage` decay used by missions, the unit
firing tail and Temporal.

With `RateDown.Reset=yes`, setting a target that is new, different or absent
zeroes the value, stage and delay accumulator. Re-setting the same target keeps
them.

## Where VERA20K owns it

`rules::phobos_rate_down` reads the four keys into the ObjectType.
`GattlingState` in `sim::combat::gattling` keeps the delay accumulator, the
"delay already elapsed" flag and the last reset target, and exposes
`update_phobos`/`idle_decay_phobos` beside the unchanged native `update`/
`idle_decay`. `Simulation::gattling_update`, the building idle-decay caller and
`Simulation::apply_rate_down_target_reset` (called from
`assign_target_represented` and the attack-move target writes) are the hook
sites.

## Native anchors

The Gattling rate-down applies in `InfantryClass_FiringAI` (`0x520AD2`, `Ext/Infantry/Hooks.Firing.cpp:13`;
its context setter `0x5206D2`, is-Gattling test `0x520AD9`) and in
`BuildingClass_Mission_Attack` (`0x44B214`, `Ext/Techno/Hooks.Firing.cpp:1277`).

## Residuals

- Native compares the target's `UniqueID` for any target, including cells and
  terrain. VERA has no unique id for those, so a cell/terrain target reads as
  "lost" and resets on every set.
- The native `GattlingRateDown(0)` call after a reset that changed the stage
  (`Ext/Techno/Body.Update.cpp:586`) has no modelled side effect; VERA plays no
  per-stage sound.
- Direct target writes outside `assign_target_represented` and the attack-move
  handler (internal AI field writes) do not run the reset.
- The three new `GattlingState` fields exist only in `phobos` builds, so a save
  does not cross between feature sets, as with the other feature-gated state.
