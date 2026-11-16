# OmniFire.TurnToTarget

An `OmniFire=yes` weapon does not turn the turret. Phobos can turn the whole
unit toward its target while it fires instead.

```ini
[General]
OmniFire.TurnToTarget=no   ; boolean, default no

[GUN]                      ; Weapon section
OmniFire.TurnToTarget=yes  ; nullable, unset -> [General]
```

Readers: the global at `Ext/Rules/Body.cpp:267` (`Valueable<bool>`,
constructor no at `Body.h:185,709`); the weapon at `Ext/WeaponType/Body.cpp:136`
(`Nullable<bool>` unset, `Body.h:66,161`).

## Native body

`UnitClass_UpdateFiring_OmniFireTurnToTarget @ 0x736E6E`
(`Ext/Unit/Hooks.Jumpjet.cpp:51-95`) runs after `GetFireError` with a code of
OK or REARM and skips when:

- the unit is warping in;
- `(Turret && !HasTurret) || TurretSpins` holds for its type — native writes
  `HasTurret` as `!Turret` (`0x747759`), so this is just `Turret ||
  TurretSpins`;
- `(DeployFire || DeployFireWeapon == wpIdx) && Mission == Unload`;
- the code is REARM and the type's `NoTurret.TrackTarget` (else
  `[General]`) resolves no;
- the fired weapon does not set `OmniFire=`;
- `OmniFire.TurnToTarget` (weapon key else `[General]`) resolves no;
- or `Locomotor->Is_Moving_Now` (`ILocomotion+0x80`) answers yes.

Otherwise it computes `DirStruct(atan2(source.Y - target.Y, target.X -
source.X))` and, when `GetRealFacing()` differs, calls `SetDesired` on the
jumpjet `LocomotionFacing` or else on `PrimaryFacing` (`FacingClass::Set_Desired
@ 0x4C9220`).

## VERA

`PhobosWeaponExtras::omni_fire_turn_to_target` and
`PhobosGlobals::omni_fire_turn_to_target` read the two keys;
`WeaponType::phobos_omni_fire_turn_to_target` resolves them. The
`FireVisit::UnitTarget` arm evaluates the hook over the pre-shot state
(`phobos_omni_fire_turn`), then keeps its answer only when the attempted shot's
`FireError` is OK or REARM (and the REARM arm's `NoTurret.TrackTarget`
resolves yes). The checks map as above, with the fired weapon and slot from
`select_weapon_against` under the same fog argument the fire path uses,
`GetRealFacing` from `body_facing_current`, the movement query from
`motion_query::is_moving_now`, and the target direction from
`facing_toward_target`. The turn is written as the frame's `hull_destination`,
which `apply_steps` applies through the same `Set_Desired` (`0x4C9220`).

## Tests

`src/sim/combat/phobos_omni_fire_tests.rs` through `commit_fire_visit`:
`the_default_key_keeps_the_hull_heading`,
`the_global_key_turns_the_hull_toward_the_target`,
`a_weapon_key_overrides_the_global`, `a_weapon_key_enables_the_turn_alone`,
`a_turreted_type_keeps_its_hull_heading`.

The turn application, the `[General]` fallback and the turret skip were each
mutated red and restored.

## Residuals

- A jumpjet now writes its `LocomotionFacing` desired heading
  (`JumpjetRuntime::set_desired_facing`) instead of the body facing, matching
  the hook's `pLoco->LocomotionFacing.SetDesired` arm; the jumpjet Update
  copies it to the hull. Covered by `a_jumpjet_turns_its_locomotion_facing`.
- `HasTurret` is not stored; it is derived from `Turret` as native derives it,
  so the gate is exact.
- The hook is evaluated before the shot and applied after it, once the fire
  error is known; native evaluates after `GetFireError` and before `Fire`. The
  facing write lands in the same frame's facing pass in both.

