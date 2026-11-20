# FireOnce.ResetSequence

An infantryman's `FireOnce=` shot clears its own target. Phobos can keep the
firing sequence and action across that clear.

```ini
[M60]                        ; [WeaponTypes] section
FireOnce.ResetSequence=no    ; Valueable<bool>, default yes
```

Reader: `Ext/WeaponType/Body.cpp:137` (constructor `true`,
`Ext/WeaponType/Body.h:67,162`).

## Native body

`TechnoClass_FireAt_FireOnce` (`0x6FF905`,
`Ext/Techno/Hooks.Firing.cpp:838-851`) runs inside the fired weapon's
`FireOnce=` tail when the firer is a Foot. For an Infantry firer whose weapon
resolves `FireOnce.ResetSequence` false it sets
`InfantryExt::SkipTargetChangeResetSequence` (`Ext/Infantry/Body.h:15`). The
tail's `Assign_Target(NULL)` (`0x6FF929`) reaches
`InfantryClass_AssignTarget_FireOnce` (`0x51B20E`,
`Ext/Techno/Hooks.Misc.cpp:845-866`): a null target with the flag set writes
`IsFiring=false`, clears the flag and jumps to `0x51B255`, skipping the class
target-change block (`0x51B20E..0x51B24F`: latch reset at Foot `+0x68D` and
the unforced idle `Do_Action`). A dead receiver (`0x51B203`, health `<= 0`)
and an unchanged target (`0x51B1FB`) skip the block before the hook, so the
flag stays set.

## VERA

`PhobosWeaponExtras::fire_once_reset_sequence` reads the key;
`combat::world_fire::fireat_tail` sets
`GameEntity::phobos_skip_target_change_reset_sequence` at the head of the
`weapon.fire_once` arm for an Infantry firer, before the shared
`assign_target_represented(None)` call. The concrete target setter
(`ConcreteMissionEffects::apply_target`) consumes it on a null target for a
live infantryman and skips `infantry_target_change_action`, so the firing
latch reset and the idle `Do_Action` do not run. VERA keeps no separate
`IsFiring` state; the skipped action request is the observable effect.

## Tests

- `src/sim/combat/combat_tests.rs` —
  `phobos_fire_once_reset_sequence_keeps_the_firing_action`: an infantryman's
  `FireOnce=yes` shot clears its target in both rows; with the key absent he
  returns to the Ready action, with `FireOnce.ResetSequence=no` he keeps the
  firing action. The skip gate was mutated red and restored.

## Residuals

- `IsFiring` itself is not modelled; only the skipped latch reset and idle
  `Do_Action` are represented.
- The flag is stored on the entity for every class; only the infantry reader
  consults it.
