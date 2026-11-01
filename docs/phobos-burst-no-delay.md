# Optional Phobos burst with no delay

Build with `--features phobos` to let a weapon empty its whole burst in one
frame. gamemd fires at most one burst shot per frame even with `ROF=` or
`BurstDelay0=` at 0; this key lifts that for the two classes that have the hook.

```ini
[SomeWeapon]                    ; rulesmd.ini
Burst.NoDelay=false             ; boolean
```

`Burst.NoDelay=yes` on a weapon with `Burst=2` or more makes an infantry or
vehicle firer launch every remaining burst shot in the same frame. The
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
  weapon carries the key, the category is infantry or vehicle, `Burst` is above
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
`FireVisit::Direct` still launches one shot.

## Residuals

- The restore after an early loop exit - the case where the last successful
  shot's `RearmTimer` value has to be written back because the next attempt was
  refused - has no fixture that reaches it deterministically. Disabling it in
  the completed-burst case is invisible, because the final shot already armed
  that value.
- `DelayedFire.Duration` is not ported, so the hooks' second clause is taken as
  satisfied (its unset branch).
