# NoTurret.TrackTarget

A turretless vehicle turns toward its target while the weapon is cooling
down, instead of waiting for the rearm to finish.

```ini
[General]
NoTurret.TrackTarget=false            ; Valueable<bool>

[SOMEVEHICLE]                         ; VehicleType
NoTurret.TrackTarget=                 ; Nullable<bool>, defaults to [General]
```

Reader `Ext/UnitType/Body.cpp:77`, declaration `Ext/UnitType/Body.h:62`;
global `Ext/Rules/Body.cpp:393`, default `Ext/Rules/Body.h:812`.

## Behavior

`UnitClass::GetFireError` (`0x740FD0`) sends only `OK` and `FACING` into the
deploy-cell block `0x741050`; every other base error returns at the
`0x7410BB` gate. Phobos `UnitClass_GetFireError_CheckFacingError`
(`0x7410BB`, 8 bytes, `Ext/Techno/Hooks.Misc.cpp:752-769`) lets a unit whose
type key is on (else `[General]`), which has no raw `Turret=`, and which is
not warping in keep going at `0x7410C3` (U5) when the base error is `REARM`.
The deploy-cell block is skipped; the rest of the routine runs. For a
non-`OmniFire` weapon an off-facing hull then answers `FACING` at
`0x74125C`, so the firing update turns it; an aligned one reaches the
`0x741300` tail and returns the retained base `REARM`, so the ROF gate still
holds. An `OmniFire` weapon skips the facing check (`0x741264`), so a keyed
unit does not turn through this hook on its own.

## Where VERA20K owns it

`rules::object_type::ObjectType::phobos_no_turret_track_target` (Vehicle
types only) with the `[General]` fallback in `rules::phobos_globals`,
resolved into `sim::combat::fire_error::FirerTypeFacts::no_turret_track_target`
by `sim::combat::fire_error_world`. `fire_error::unit` holds the hook's REARM
arm; its U13 tail returns the base when the locomotor's `Can_Fire` admits.

`UnitClass_UpdateFiring_OmniFireTurnToTarget` (`0x736E6E`,
`Ext/Unit/Hooks.Jumpjet.cpp:51-103`) also reads this key, as the REARM guard
for an `OmniFire` weapon with `OmniFire.TurnToTarget`; that hook is implemented
by the OmniFire chain (`sim::combat::{world_fire,phobos_omni_fire_tests}`,
including the jumpjet `LocomotionFacing` arm), so this key's part has no
residual.
