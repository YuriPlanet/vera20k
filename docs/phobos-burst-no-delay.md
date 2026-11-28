# Optional Phobos burst with no delay

Phobos is in every build; turn it on from a game's data to let a weapon empty its whole burst in one
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
  stays one shot per call. When the weapon carries the key, the delayed-fire gate
  above is open, the category is infantry or vehicle, `Burst` is above 1 and the
  shot left a live burst index, it hands the admitted shot back to its caller.
  The two class firing updates - `FireVisit::UnitTarget` and
  `FireVisit::InfantryTarget` in `visit_fire`, the sites Phobos hooks - then run
  `fire_remaining_burst_this_frame`, which is the hook's own loop
  (`for (int i = pThis->CurrentBurstIndex; i < pWeapon->Burst &&
  pThis->GetFireError(...) == FireError::OK && pThis->Fire(...); ++i)`):
  it ends immediately when the first shot left `CurrentBurstIndex` at 0
  (`Hooks.Firing.cpp:189`), otherwise it repeats `TechnoClass::Fire` while a
  **local** counter stays below `Burst`. The counter is local because the shot
  itself may reset `CurrentBurstIndex` - `SetChargeTurretDelay`'s sibling
  `ForceFullRearmDelay` reset in `TechnoClass_FireAt_LateLogic` - and that neither
  shortcuts nor extends the loop.

  Every repeat is a real admission, not a clone of the first shot: `repeat`
  tells `admit_attacker_fire` to skip the class firing-frame block (the
  `0x5209AF` stage equality and its `Burst.FireWithinSequence`
  `RandomRanged(3, 5)` draws, which belong to the class update, not to `Fire`)
  and the infantry firing-action start, while `GetFireError` still runs. That
  keeps the repeat behind the ammo, range and legality gate native asks again
  for each shot, and keeps the second shot's own weapon/rearm arithmetic.
  Every other FireAt caller (a mission's `FireVisit::Direct`, retaliation,
  aircraft release) sees the ordinary one shot per call.

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
opens the repeat again. The last two meet the
[Burst.FireWithinSequence](phobos-burst-sequence.md) gate: a visit with both keys
and a listed `Burst.Delays` fires every shot, consumes only the completing
shot's own rearm jitter and never re-enters the sequence test; a burst that
starts on index 1 also arms the gate's overrun in this fixture (no ART sequence
record, so the projected frame is past a zero-frame sequence), so its shot
resets `CurrentBurstIndex` to 0 and the hook's own `if (!CurrentBurstIndex)`
check ends the visit after that one shot, with the retained stage untouched.

## Residuals

- The delayed-fire subsystem this page used to call unported **is ported**;
  `docs/phobos-delayed-fire.md` owns it. `HandleDelayedFireWithPauseSequence`
  (`src/Ext/Techno/Body.cpp:699-749`) is
  `sim::combat::world_fire::phobos_delayed_fire_pause_sequence`
  (`src/sim/combat/world_fire.rs:617`) reached from `commit_fire_visit` (`:974`,
  `:1292`), with the saved `DelayedFireTimer` / `DelayedFireWeaponIndex` /
  `DelayedFireSequencePaused` fields on the firer (`start_phobos_delayed_fire`,
  `:730`, cleared at `:741`). The pause flag is consumed by
  `TechnoClass_AI_AnimationPaused` (`0x6FABC4`,
  `src/sim/world/techno_ai.rs:3078`), which skips the generic Stage block, and
  it is hashed (`src/sim/world/world_hash.rs:1754`). This page itself reads only
  `DelayedFire.Duration` and `DelayedFire.OnlyOnInitialBurst`, for the repeat
  gate that keeps native's one shot per frame. Its remaining residuals
  (`CenterAnimOnFirer` using the object coordinate, `ExtraRange.Prefiring`) live
  on the delayed-fire page, not here.
