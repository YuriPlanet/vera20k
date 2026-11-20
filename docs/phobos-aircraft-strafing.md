# Aircraft strafing keys

Phobos `WeaponTypeExt::LoadFromINIFile` reads five strafing keys on a weapon
(`src/Ext/WeaponType/Body.cpp:108-112`); three of them fall back to `[General]`
`Valueable<bool>` globals (`src/Ext/Rules/Body.cpp:264-266`, constructor
`Body.h:706-708`, all false). The readers live in
`src/rules/phobos_strafing.rs` and `src/rules/phobos_globals.rs`.

| Key | Type | Meaning |
| --- | --- | --- |
| `Strafing.Shots=` | `Nullable<int>`, `.Get(5)` | bombs one strafing round fires, and the spy plane's photo cap |
| `Strafing.SimulateBurst=` | `Nullable<bool>`, global fallback | alternate `CurrentBurstIndex` across a single-shot weapon's strafing passes |
| `Strafing.UseAmmoPerShot=` | `Nullable<bool>`, global fallback | one Ammo per shot instead of one per round |
| `Strafing.TargetCell=` | `Nullable<bool>`, global fallback | fire the round at the Target's cell instead of the Target |
| `Strafing.EndDelay=` | `Nullable<int>`, unset derives | frames after the round's last shot |

## Where VERA20K owns it

- `rules::phobos_strafing::PhobosStrafingRules` holds the five keys on
  `WeaponType::phobos_strafing()`; `rules::phobos_globals::PhobosGlobals`
  holds the three `[General]` fallbacks. `RuleSet::simulation_config_hash`
  adds a `phobos-weapon-strafing-v1` section only when a key is authored, so
  stock rules and saves keep their identity.
- The per-aircraft state is `sim::mission::leaf::AircraftMissionLeaf`:
  `strafe_bombs_dropped_this_round` (constructor 0) and `strafe_target_cell`
  (constructor null), serialized with the rest of the leaf. `mission::leaf`
  owns both; the getters and setters are the only access.
- `sim::aircraft::mod::attack_visit` runs the entry hook through
  `attack_mission::strafe_entry` before the state body.
  `attack_mission::strike_visit` owns every shot, delay and round-end
  decision through its `StrikeHost`, implemented by
  `sim::combat::aircraft_release::CombatStrike`. That receiver's
  `fire_weapon_shot` and `fire_weapon_tail` are the one port of
  `AircraftExt::FireWeapon`, called by `release`, `fire_at` and
  `fire_at_cell`; `shot_delay` is `GetDelay`. All read the `StrafeKeys`
  snapshot of weapon 0 (`StrafeKeys::from_weapon`, globals applied).
- `sim::aircraft::spyplane_mission::camera_count_allows` gates both camera
  blocks at `snapshot`.

## RNG, timers and detaches this chain passes

- RNG: the entry hook, `GetDelay`, the TargetCell store and the camera count
  draw nothing. A shot's emission and the existing `Scatter` draw exactly
  what the stock shot already drew; the unported cell `ScatterContent`
  (residual below) would add that scatter's draws.
- Timer writes: each visit returns the weapon's `ROF=` or, when `GetDelay`
  ends the round, `Strafing.EndDelay` or `(Range + 4 * LeptonsPerCell) /
  Speed`, which `MissionClass::AI` restarts the mission timer with.
- Detaches: `Strafing.UseAmmoPerShot` at Ammo 0 clears the Target
  (`SetTarget(nullptr)`) and the NavCom destination (`SetDestination(nullptr,
  1)`); a following cell shot re-assigns the TargetCell as the destination
  (`SetDestination(cell, 1)`), and the round end clears `Strafe_TargetCell`.

## Mission_Attack

`AircraftClass::Mission_Attack` (`0x00417FE0`) states 4..9 are VERA's
`sim::aircraft::attack_mission::strike_visit`, run through
`sim::combat::aircraft_release`, where every shot is emitted.

- **Entry hook** (`AircraftClass_Mission_Attack_StrafeShots`, `0x417FF1`,
  `src/Ext/Aircraft/Hooks.cpp:25-50`): on every visit a state outside
  `FireAtTarget2_Strafe..FireAtTarget5_Strafe` (AirAttackStatus 6..9) resets
  `Strafe_BombsDroppedThisRound`; with shots fired, a `Strafing.Shots` above 5
  sends `FireAtTarget3_Strafe` (7) back to `FireAtTarget2_Strafe` (6) while
  `shots - 3 - counter > 0`. `sim::aircraft::attack_visit` runs it before the
  state body.
- **Release and shots** (`AircraftExt::FireWeapon`,
  `src/Ext/Aircraft/Body.cpp:12-47`): each strafing weapon pass increments the
  round counter; with `UseAmmoPerShot` it debits one Ammo, clears
  `ShouldLoseAmmo` (`+0x6C8`) and, at Ammo 0, clears the Target and
  Destination. With `Burst < 2` and `SimulateBurst`, `CurrentBurstIndex` is
  written to `bombDropCount % 2 == 0` before the shot. The state-4 release
  (`0x418403`) and every strafe-state shot (`0x4186B6`, `0x418805`,
  `0x418914`, `0x418A23`, `0x418B1F`) call this one owner.
- **GetDelay** (`src/Ext/Aircraft/Hooks.cpp:211-228`, hooks `0x4184CC`,
  `0x418506`, `0x418883`, `0x418992`, `0x418AA1`, `0x418B8A`): a shot's delay
  is the weapon's `ROF=`, unless it is the last shot, the counter equals
  `Strafing.Shots`, or `UseAmmoPerShot` left Ammo 0. That ending clears
  `Strafe_TargetCell`, moves to `AirAttackStatus::FlyToPosition` (3) and takes
  `Strafing.EndDelay` or `(Range + 4 * LeptonsPerCell) / Speed`, the same
  wrapping sum and signed division as stock state 9 (`0x00418B8A`).
- **TargetCell store** (`0x4184CC`, `Hooks.cpp:233-241`): on the state-4
  strafing suffix the Target's `GetCoords()` cell goes into
  `Strafe_TargetCell` when the key (or its global) is on.
- **StrafeCell** (`0x41879D`/`0x4188AC`/`0x4189BB`/`0x418ACA`,
  `Hooks.cpp:245-266`): states 6..9 with a stored cell ask
  `GetFireError(cell, index, true)`; `OK`, `FACING`, `CLOAKED` and `RANGE`
  fire at the cell, assign it as the Destination, increment the state and take
  `GetDelay` (the fifth shot's increment reaches 10, then the last-shot delay
  moves to 3). Other codes poll a frame, as the stock `0x418BC5` tail does.
- **Spy plane** (`CheckSpyPlaneCameraCount`, `src/Ext/Aircraft/Hooks.cpp:1164-1179`,
  hooks `0x415666`/`0x4157EB`): only an explicitly authored `Strafing.Shots`
  caps the snapshots; once the round counter reaches it the snapshot is
  skipped, otherwise it increments.

## Residuals

- `Strafing=` (the classifier, `src/Ext/WeaponType/Body.cpp:107`,
  `IFlyControl::Is_Strafe`) is out of this chain. Trigger: a weapon that
  authors it (no retail weapon does). Effect: VERA's projectile-`ROT`
  classification still decides. Frequency: per shot of that weapon.
- `AircraftExt::FireWeapon`'s `Burst` loop in states 5..9
  (`Body.cpp:20-24`) fires the whole burst per strafe-state shot natively;
  VERA keeps the stock one `Fire` per state. Trigger: a strafing weapon with
  `Burst>1` (no retail one). Effect: fewer bullets per pass. Frequency: every
  strafe-state visit.
- The cell `ScatterContent` after a StrafeCell shot (`Hooks.cpp:257-258`),
  gated by `AircraftTypeExt::FiringForceScatter` and
  `[General] AircraftFiringForceScatter`, is not ported with these keys.
  Trigger: a `TargetCell` round. Effect: units on the cell do not scatter
  away from the aircraft. Frequency: every cell shot.
- `CurrentAircraftWeaponIndex`: VERA selects the weapon again for each shot
  and reads the primary slot's keys for the round facts. Trigger: a
  multi-weapon aircraft whose selection changes mid-round. Effect: the round
  uses the primary slot's `Strafing.*`. Frequency: none in retail (one weapon
  slot).
