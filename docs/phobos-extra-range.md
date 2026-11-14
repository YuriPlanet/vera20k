# Phobos `ExtraRange.*` chain

Keys (all `[General]`): `ExtraRange.TargetMoving`,
`ExtraRange.TargetMoving.CloseRangeOnly`, `ExtraRange.FirerMoving`,
`ExtraRange.Prefiring`, `ExtraRange.Prefiring.IncludeBurst`
(`Ext/Rules/Body.cpp:511..515`, defaults `Ext/Rules/Body.h:925..929`).
The three range keys are `Valueable<Leptons>`, parsed as
`cells × 256` (`Parser<Leptons>`, `Utilities/TemplateDef.h:487..493`), so
`ExtraRange.FirerMoving=1` is one cell. `IncludeBurst` is a `Valueable<bool>`
defaulting true.

## Native evidence

`TechnoClass_InRange_WeaponRange @ 0x6F7248`
(`Ext/Techno/Hooks.WeaponRange.cpp:133..180`) reimplements
`TechnoClass::InRange`'s range query: with no live `KeepRange`
(`GetTechnoKeepRange`, `:145..148`) it starts from
`GetRangeWithModifiers` and, when that is not the -512 always-in-range
sentinel, adds:

- `ExtraRange.Prefiring` when `IsPrefiring` (`:152..155`);
- `ExtraRange.TargetMoving` when the target is `IsChasing` (`:157..162`);
  the source's `CloseRangeOnly` arm is unreachable because the per-weapon
  `ExtraRange_TargetMoving` it tests for `isset()` is itself read from
  `[General]` (`Ext/WeaponType/Body.cpp:200`), so an authored global always
  takes the per-weapon branch and skips the `CloseRange` test. VERA
  reproduces that behavior and still reads the key;
- `ExtraRange.FirerMoving` when the firer `IsMovingFire` (`:164..168`).

`IsChasing`/`IsMovingFire` ask the Foot locomotor's `Is_Really_Moving_Now`
(`:43..72`). `IsPrefiring` (`:74..131`) is true when the weapon's
`ExtraRange_Prefiring_IncludeBurst` (default true) and
`CurrentBurstIndex % Burst != 0`, or the delayed-fire timer runs, or the
class fires: Unit `CurrentFiringFrame >= FiringSyncFrame{0,1}`, Aircraft
attack-fire statuses, Building `DelayBeforeFiring`/`IsFiringNow`, Infantry
`IsFiring`.

Phobos's per-weapon copies (`Ext/WeaponType/Body.cpp:200..203`) read the
same spellings from `GameStrings::General`, so they always mirror the globals;
VERA owns the globals only.

## Implementation

- `rules::phobos_globals` adds the five values with the constructor defaults;
  `IncludeBurst` starts true.
- `sim::combat::extra_range::extra_range_leptons` sums the applicable
  bonuses; `is_prefiring` implements the burst arm (guard `Burst > 0`).
- `sim::combat::in_range::compute_range_target` adds the sum to the
  modified weapon range before every other InRange term, only when no live
  `KeepRange` maximum replaced it and the sentinel is not in play. The
  terrain cell-target gate (`cell_target_in_range`) threads its caller's
  world handle, so attack-ground cells take the same bonuses.

## Residuals

- `IsPrefiring`'s delayed-fire timer and the Unit/Aircraft/Building/Infantry
  firing-state arms need state VERA does not own yet (the delayed-fire
  subsystem and the per-class firing latches), so they answer false.
- The mapless range path (`mapless_in_range`, used without resolved terrain)
  has no `Simulation` handle and therefore adds no ExtraRange bonuses.
- Phobos's per-weapon ExtraRange fields are dormant by construction: they are
  read from `[General]`, so a weapon section can never override them.
- Retail rules author none of the keys, so stock behavior is unchanged.
