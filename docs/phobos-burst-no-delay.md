# Optional Phobos burst with no delay

Build with `--features phobos` to let a weapon empty its whole burst in one
frame. gamemd fires at most one burst shot per frame even with `ROF=` or
`BurstDelay0=` at 0; this key lifts that for the two classes that have the hook.

```ini
[SomeWeapon]                    ; rulesmd.ini
Burst.NoDelay=false             ; boolean
```

`Burst.NoDelay=yes` on a weapon with `Burst=2` or more makes an infantry or
vehicle firer launch every remaining burst shot in the same frame. Both hooks
carry a second condition - `!DelayedFire.Duration.isset() ||
DelayedFire.OnlyOnInitialBurst` (`src/Ext/Unit/Hooks.Firing.cpp:125`,
`src/Ext/Infantry/Hooks.Firing.cpp:189`) - so a weapon that authors
`DelayedFire.Duration=` without `OnlyOnInitialBurst=yes` keeps the ordinary one
shot per frame. The
`BurstDelay0..3` gaps are skipped, not shortened: the firer ends the frame with
the rearm value that the *last* shot armed, which is the weapon's full `ROF`, so
the next burst starts after a normal reload. Only infantry and vehicles are
affected — the feature has no building or aircraft hook, and aircraft already
release their whole burst within one visit.

## Native site

Phobos patches the two class firing updates,
`UnitClass_UpdateFiring_BurstNoDelay @ 0x736F67` and
`InfantryClass_UpdateFiring_BurstNoDelay @ 0x5209EE`
(`src/Ext/Unit/Hooks.Firing.cpp:111`, `src/Ext/Infantry/Hooks.Firing.cpp:175`).
The gate is `WeaponType->Burst > 1` plus `WeaponTypeExt::Burst_NoDelay`
(`Valueable<bool>`, `Ext/WeaponType/Body.cpp:122`, default false at `Body.h:149`)
and a `DelayedFire.Duration` clause. Inside it the hook calls `TechnoClass::Fire`
itself; if that shot already completed the burst it skips the stock code,
otherwise it records `RearmTimer.TimeLeft`, starts the timer at 0 and loops
`Fire` while `CurrentBurstIndex` has not wrapped, `GetFireError` still answers OK
and the launch succeeded, capturing each shot's `TimeLeft` as it goes. After the
loop it restarts `RearmTimer` with the last captured value.

## Where VERA20K owns it

- `rules/weapon_type.rs` - the `Burst.NoDelay=` read (`read_ares_bool`, the
  `Valueable<bool>` semantics) on `WeaponType`.
- `sim/combat/world_fire.rs` - `resolve_attacker_fire` is `TechnoClass::Fire` and
  stays one shot per call, but it reports `FireReach::burst_repeat` when the
  weapon carries the key, the delayed-fire gate above is open, the category is
  infantry or vehicle, `Burst` is above
  1, the shot stepped the burst and the burst did not complete. The two class
  firing updates - `FireVisit::UnitTarget` and `FireVisit::InfantryTarget` in
  `visit_fire`, the sites Phobos hooks - answer that by running
  `fire_remaining_burst_this_frame`, which zero-arms `RearmTimer` between
  attempts and restores the last shot's value after the loop. Every other
  FireAt caller (a mission's `FireVisit::Direct`, retaliation, aircraft release)
  sees the ordinary one shot per call.

## Tests
`sim::combat::burst_no_delay_tests` drives one class firing update per frame for
both categories: with the key a `Burst=3` weapon launches three projectiles in
that single visit, the burst index wraps to 0 and `RearmTimer` ends on the full
ROF; without it the same visit launches one, the index steps to 1 and the
mid-burst gap stands. A third test pins the placement: the same weapon through
`FireVisit::Direct` still launches one shot. A fourth reaches the restore after a
*refused* attempt: a firer with `Ammo=1` spends its round on the first shot,
`GetFireError`'s ammo arm then refuses the repeat, and removing the write-back
fails that test alone while the completed-burst test still passes. A fifth gates
on the delayed-fire keys: `DelayedFire.Duration` with `OnlyOnInitialBurst` unset
holds the cadence to one shot per frame, and adding `OnlyOnInitialBurst=yes`
opens the repeat again.

## Residuals

- The delayed-fire subsystem is unported: `HandleDelayedFireWithPauseSequence`
  (`src/Ext/Techno/Body.cpp:699-749`, called from the unit firing update at
  `src/Ext/Unit/Hooks.Firing.cpp:89` and `:103` and the infantry one at
  `src/Ext/Infantry/Hooks.Firing.cpp:118`), its nine other `DelayedFire.*` keys,
  the saved `DelayedFireTimer` / `DelayedFireWeaponIndex` /
  `DelayedFireSequencePaused` fields, the `GetRangedRandomOrSingleValue` draw and
  `CreateDelayedFireAnim`. VERA reads `DelayedFire.Duration` and
  `DelayedFire.OnlyOnInitialBurst` only for the repeat gate, so a weapon with
  `Duration` set keeps native's one shot per frame but neither pauses its firing
  sequence nor plays the delayed-fire animation.
