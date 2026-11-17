# Burst shots within the firing sequence

Build with `--features phobos` to let a weapon fire its `Burst=` shots at
frames inside one infantry firing sequence instead of one shot per frame.

```ini
[SomeWeapon]                        ; rulesmd.ini
Burst.FireWithinSequence=false      ; boolean, default no
```

The first shot stays on the type's stock firing frame (`FireUp=` and friends
on the infantry art, or the unit art `FireUp=` when the unit is an SHP,
non-turreted body). Each later shot waits for the cumulative `Burst.Delays`
of the shots before it: `Burst.Delays=2,0` fires the second shot two frames
after the first and the third three frames after the first (a listed zero is
clamped to one frame after index 0, because the sequence advances once per
frame). An index without a list entry draws `RandomRanged(3, 5)`, one draw per
such index, in index order; only index 0 is guaranteed to answer without a
draw. When the next shot's projected frame would fall past the current
sequence's frame count, the shot arms the full rearm and the burst index
restarts at 0, so the weapon reloads normally instead of waiting for a frame
that never comes. Ceasing fire mid-sequence leaves the burst counter where it
was; the next sequence recomputes the cumulative frames from the top.

## Native site

`InfantryClass_FiringAI`'s stage comparison at `0x5209AF` is replaced by a hook
(`src/Ext/Infantry/Hooks.Firing.cpp:83-137`) that computes
`cumulativeDelay` over `i = 0..=CurrentBurstIndex` from
`WeaponTypeExt::GetBurstDelay(i)` (`Ext/WeaponType/Body.cpp:62-74`; see the
[Burst.Delays](phobos-burst-delays.md) page), draws `RandomRanged(3, 5)` for a
`-1` answer, clamps non-initial delays to 1 and compares the animation stage
with `fireUp + cumulativeDelay`. On a match it also sets
`TechnoExt::ForceFullRearmDelay` when `fireUp + projectedDelay` exceeds the
sequence's frame count. The unit hook at `0x736F61`
(`src/Ext/Unit/Hooks.Firing.cpp:7-109`) does the same for a turretless
`Voxel=no` unit, but only when `UnitTypeExt::FireUp` (art `FireUp=`, default
-1) is authored. The two owners differ in where the frame comes from: the
infantry hook reads the live firing action's frame count, the unit hook reads
the ART `FiringFrames=` through the SHP sequence and keeps the frame on
`UnitClass+6C0`.

`ForceFullRearmDelay` is consumed at `0x6FD054` in GetROF
(`src/Ext/Techno/Hooks.Firing.cpp:1132-1145`): the mid-burst branch is skipped
for the ordinary full-rearm arm, so that shot arms the whole `ROF=` instead of
a burst gap. The shot's burst step clears the flag and restarts the index at 0
(`TechnoClass_FireAt_LateLogic`, `0x6FF660`,
`src/Ext/Techno/Hooks.Firing.cpp:902-910`).

## Where VERA20K owns it

- `rules/weapon_type.rs` - the `Burst.FireWithinSequence=` read
  (`Valueable<bool>`, `Body.cpp:121`, `Body.h:53/148`) and the existing
  `phobos_burst_delay` `GetBurstDelay` owner it reuses.
- `sim/combat/world_fire.rs` - `phobos_burst_sequence_delays` walks the burst
  with the Scenario RNG exactly as the hooks do; both infantry admission sites
  (the retained sequence and the one that starts the action) compare the stage
  with `fireUp + cumulativeDelay` and set the full-rearm flag on an overrun.
  `Burst.NoDelay`'s repeat (`fire_remaining_burst_this_frame`) launches the shot
  the first pass was admitted on through `emit_admitted_fire` instead of asking
  again, so a weapon authoring both keys fires its whole burst inside the
  admitted sequence frame, without re-running the stage test or its
  `RandomRanged(3, 5)` draws.
- `sim/combat/burst.rs` - `WeaponBurst` carries the `ForceFullRearmDelay`
  equivalent: the flag is set by admission, read by GetROF, and cleared by
  `complete_shot`, which also restarts the index at 0.
- `sim/combat/rof.rs` - `get_rof`'s `0x6FD054` bypass leaves the mid-burst
  branch while the flag is set. The stock kernel and the Ares/native control
  queries stay identical when the feature is off.

The unit arm of `0x736F61` has its own owners. No retail unit art authors
`FireUp=`, so the arm is dormant in retail data and reached only by
Phobos-authored art:

- `rules/art_data.rs` - the ART `FireUp=` read
  (`Ext/UnitType/Body.cpp:93`, `Valueable<int>`, `Body.h:66/127`, default -1).
  It is not the infantry `FireUp=` discharge frame, which
  `read_infantry_discharge_frames` owns.
- `rules/object_type.rs` + `rules/ruleset.rs` - `project_art_data` copies the
  key from the type's `ImageFile` ART section into `phobos_fire_up`, the way
  it already copies `TurretBarrel`.
- `sim/animation.rs` - `UnitFiringCounter` (`UnitClass+6C0`) is the
  `CurrentFiringFrame` owner; `seed_sequence_frame` is native's `== -1`
  initialisation.
- `sim/combat/world_fire.rs` - `phobos_unit_fire_up_skips` runs in
  `FireVisit::UnitTarget` after the hull facing update and before
  `resolve_attacker_fire`, which is where `0x736F61` sits relative to
  `Fire_At_Target`; a held frame leaves `fire_error` at its `NoTarget` default,
  so the `0x737063` tail keeps the stock gattling path. The raw
  `FireVisit::Direct` (`FireAt`) path stays ungated.
  `FireUp.ResetInRetarget` and its target-setter reset already live in
  `sim::mission`.

## Tests
`sim::combat::phobos_burst_sequence_tests` drives the infantry firing update
through production code. `listed_delays_move_the_admission_frames_without_draws`
fires at `fireUp + 0/2/3` for `Burst.Delays=2,0` with no Scenario draw;
`unset_entries_draw_one_value_per_index_in_order` pins the draw count and the
frame the drawn values produce; `overrun_arms_the_full_rearm_and_restarts_the_burst`
covers the past-the-sequence arm (index restarts at 0, rearm is the full ROF);
`a_frame_inside_the_sequence_keeps_the_mid_burst_delay` is its control;
`the_key_off_keeps_the_stock_stage` shows the key changes nothing without the
option; `a_unit_without_the_art_key_stays_stock` pins the -1 unit default.

`sim::combat::phobos_unit_fire_up_tests` drives the unit arm through
`FireVisit::UnitTarget`. `fire_up_holds_an_shp_unit_until_the_authored_frame`
pins the `-1` seed, a held frame and the authored frame;
`moving_clears_the_fire_up_sequence_unless_opportunity_fire` covers
`Is_Really_Moving_Now` with the key off and on;
`burst_overrun_forces_a_full_rearm` covers the projected overrun and shows the
shot takes the full ROF; `the_key_off_keeps_the_stock_unit_cadence` is its
control.

`sim::combat::burst_no_delay_tests` covers the crossing with `Burst.NoDelay`:
`burst_no_delay_fires_all_shots_without_redrawing_the_sequence` fires a `Burst=3`
weapon's whole burst in one visit, and with `Burst.Delays` listed the only
Scenario value that visit consumes is the completing shot's own stock GetROF
jitter — the gate draws nothing, and a one-shot visit never arms that rearm at
all. `burst_no_delay_repeat_does_not_restart_the_sequence_gate` starts the burst
on index 1, keeps the stage that admission matched (a re-admission would first
restart the firing action and move it) and still fires the remaining shot. Both
are written to fail while the repeat re-admits.

## Residuals

- The unit arm's `ForceFullRearmDelay` overrun needs `CurrentFiringFrame < -1`.
  VERA's `UnitFiringCounter::advance` stops at -1, and every frame a live
  counter decodes is at most `frames / 2`, so `fireUp + projected` stays inside
  `frames` and the overrun never arms on its own. Trigger: an authored
  `FireUp=` plus `Burst.FireWithinSequence=`. Effect: no full rearm from that
  arm; the shot still fires on its sequence frame. The arm is ported for the
  native state a counter below the floor can carry, and the test drives it
  with that prior.
- A unit with `FireUp=` above 0 whose ART leaves `FiringFrames=` unset fires
  stock in VERA but holds every frame in native. Native skips its `frames >= 0`
  seed arm, so `CurrentFiringFrame` stays -1 and `(frames - (-1)) / 2` reads 0
  for `frames <= 0` — always below the authored frame. VERA returns before the
  gate when `unit_shp_read_state` answers no record, because a non-positive
  `FiringFrames` has no frame owner yet. Trigger: such a unit; retail authors
  no unit `FireUp=` at all (none of the 101 `FireUp=` art sections in
  `ARTMD.INI` belongs to a `[VehicleTypes]` entry), so this stays dormant.
  Effect: VERA launches on frame 0 where native would never launch. `FireUp=0`
  is not affected: native's frame 0 matches.
- `OmniFire.TurnToTarget` while a `FireUp=` frame holds the shot: native's
  `UnitClass_UpdateFiring_OmniFireTurnToTarget @ 0x736E6E` runs before
  `UnitClass_UpdateFiring_FireUp @ 0x736F61`, so the hull or jumpjet facing
  turns even when the sequence holds the shot. VERA evaluates the turn before
  the gate but applies it only on the non-held path. Trigger: a unit authoring
  both an SHP `FireUp=` and `OmniFire.TurnToTarget=yes`; retail authors no unit
  `FireUp=`, so this stays dormant. Effect: VERA keeps the previous facing on
  held frames.
- `LastRearmWasFullDelay` (the other Phobos rearm flag, read by charge-turret
  AttachEffect) is not kept by VERA; `sim::attach_effect` records that residual
  with the charge timer.
- `DelayedFire.PauseFiringSequence` ordering: native calls
  `HandleDelayedFireWithPauseSequence` inside the `0x5209AF` hook before the
  `frame == firingFrame` test and hands it this cumulative frame. VERA's
  DelayedFire suppression lives in `admit_attacker_fire` after the stage test,
  so with both keys armed the timer's completed-arm reset can happen one
  matching frame later than native. Trigger: a weapon authoring both
  `DelayedFire.PauseFiringSequence=yes` and `Burst.FireWithinSequence=yes`;
  effect: a delayed burst shot may skip or repeat one pause step. Retail sets
  neither key.
- The completed burst's rearm takes one Scenario jitter draw:
  `TechnoClass_RearmDelay_ROF` (`0x6FD0B5`, six replaced bytes covering the
  stock random call, `SkipGameCode=0x6FD0BB`) *replaces* that draw with
  `GetRangedRandomOrSingleValue(ROF.RandomDelay)`; the `{0, 2}` default is the
  vanilla jitter. `sim::combat::rof::get_rof` therefore draws the stock range
  only when no `ROF.RandomDelay` range answers.
