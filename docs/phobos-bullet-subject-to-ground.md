# Projectile subject to ground (`SubjectToGround`)

A BulletType-level Phobos key: the *firing techno's* pre-shot obstacle scan
stops at the first point where the projectile would pass below the cell floor.
It is unrelated to `Trajectory.Straight.SubjectToGround` /
`Trajectory.Bombard.SubjectToGround`, which detonate a projectile already in
flight (see [trajectories](phobos-trajectory-straight.md)).

## Key

| Key | Owner | Type | Default |
|---|---|---|---|
| `SubjectToGround=` | BulletType | `Valueable<bool>` | `no` |

Read: `BulletTypeExt::LoadFromINIFile` (`Ext/BulletType/Body.cpp:56`, field
`Ext/BulletType/Body.h:59`, constructor `:123`). VERA reads it in
`src/rules/projectile_type.rs` next to `Trajectory`; the trajectory-level keys
stay in `src/rules/phobos_trajectory.rs`.

## Behaviour

`TechnoClass_InRange` ends in `BulletObstacleHelper::FindFirstImpenetrableObstacle`
(hook `0x6F7647`, `Ext/Bullet/Hooks.Obstacles.cpp:222-237`). That helper walks
the source→target line cell by cell (`FindFirstObstacle`, `:25-66`) and, with
`subjectToGround` set, stops at the first sample whose

```
crdCur.Z < MapClass::GetCellFloorHeight(crdCur)
```

after the cell predicate at `:40-50`. A blocked cell makes `InRange` refuse the
shot, so the unit reports "not in range" instead of firing through the terrain.

VERA's walk is `sim::combat::line_of_fire::walk_line_for_blocking_cell`, the
0x004CC100 body. `LineOfFireShot` carries the flag and the loop performs the
same below-floor test after `cell_blocks_line_of_fire`, returning the sample's
own cell (the native `pCellCur`). With the key unset the walk is byte-for-byte
the stock one.

## Residuals

- Phobos replaces the *whole* `TechnoClass::InRange` obstacle call with
  `FindFirstObstacle`, whose gate is
  `SubjectToLand.isset() || SubjectToWater.isset() ? true : BulletType->Level`,
  or `subjectToGround` (`Ext/Bullet/Hooks.Obstacles.cpp:41,82-88`). **Ported for
  this call site.** VERA runs the stock cell verdict and the Phobos cell
  predicate over one sample sequence (`LineOfFireShot::vanilla_scans` /
  `phobos_scans`, `phobos_terrain_blocks`): `SubjectToLand`/`SubjectToWater`
  enter the walk on their own and the cell's own `LandType` (`Water`/`Beach` →
  water key, everything else → land key) picks which applies. `InRange` passes
  `isTargetingCheck = true`, so `.Detonate` cannot veto there, and the `Level`
  floor branch is guarded by `!isTargetingCheck`, so it cannot fire on this arm
  (it belongs to the bullet's flight). Test
  `subject_land_and_water_put_a_projectile_into_the_walk`.
- The FLH-predicted source offset (`AddFLHToSourceCoords`, `:89-125`) is
  **ported for this call site**. `sim::combat::in_range::phobos_scan_input`
  keeps `subjectToGround` only for a firer that is not `IsInAir()`, against a
  target that is not `IsInAir()`, with no `Transporter` (`:92-97`); any miss
  clears the flag and keeps the raw source. Otherwise it builds
  `identity → RotateZ((float)-atan2(dy, dx)) → ApplyTurretOffset` (only under
  `HasTurret()`) `→ Translate(FLH.X, Burst == 1 ? FLH.Y : 0, FLH.Z)` over the
  *cell's* own coordinate (`OnBridge` adds the deck rise) plus
  `((int)x, -(int)y, (int)z)` — `util::flh_transform::phobos_flh_source`. Two
  terms differ from `GetFLH`: the heading is predicted from the target vector
  instead of the retained facing, and a `Burst` above 1 centres the lateral term
  instead of alternating it. `InRange` now runs two passes, as native does: the
  stock walk from the raw source first, then this one from the predicted source
  (`PhobosScanInput`, `walk_arm`). Residuals: the weapon FLH comes from the slot
  whose weapon id matches the one `InRange` checks (native reads
  `GetWeapon(SelectWeapon(target))->FLH`), and the `TurretOffset` pivot is the
  art triple without a building's installed-upgrade `HasTurret`.
- `FindFirstObstacle` samples `maxDelta + isTargetingCheck` points (`:64`), so
  `InRange` also tests the target's own cell; VERA's walk stops at its
  existing sample count, which has not been compared against that bound.
- After the scan, the same hook runs the spawner's position search (`:239-252`):
  a null result with `SpawnManager`, the Foot bit and `!IsInAir()` moves the
  firer's `Location` to the source coordinate, asks `0x703B10`
  (`TechnoClass::IsOnBridge_ForFiring`) and turns a `true` answer into the
  source cell as the obstacle. **Ported**: `in_range::phobos_spawner_obstacle`
  gates on VERA's own `spawn_manager` state, the Foot category bit and
  [`entity_layer`]'s AIR answer, then asks
  `in_range::is_on_bridge_for_firing_at` at the shot's *source* coordinate —
  the native predicate is coordinate-based because `OnBridge` (`+0x8C`) does
  not follow `Location`, so no entity is mutated. The two cheap gates run
  before any cell read, so an ordinary shooter adds no lookups to the traced
  query order.
- The bullet's own flight arm (`0x468C86` `BulletClass_ShouldExplode_Obstacles`,
  `:130-160`) calls `GetObstacle` with `isTargetingCheck = false`, where
  `.Detonate` decides between exploding and flying on. **Ported for the two
  nullable keys**: the in-flight probe
  (`sim::world::projectile_collision::cliff_wall`) now feeds the type's four
  keys into `LineOfFireShot` and asks `phobos_terrain_blocks(&cell, false)`
  before the stock cell verdict, so `SubjectToLand`/`SubjectToWater` detonate
  mid-flight and `.Detonate=no` lets the bullet fly through the cell it matches
  (`line_of_fire.rs::the_flight_arm_reads_detonate_and_the_range_arm_does_not`).
  The helper's `Level` floor branch (`Level && IsOnFloor()`, `:97-102`) is not
  duplicated here: VERA's stock `projectile.collision().level` arm in the same
  `shared()` already answers it.
- The scan feeds only `InRange`, as natively; the bullet's own flight is
  governed by the trajectory keys.

## Tests

- `src/sim/combat/line_of_fire.rs` —
  `subject_to_ground_stops_the_walk_below_the_floor`: a sample at `Z = -16` over
  flat ground blocks only when the flag is set; `Z = 0` never blocks.
- `src/sim/combat/line_of_fire.rs` —
  `subject_land_and_water_put_a_projectile_into_the_walk`: `SubjectToLand` stops
  a walk over a land row and leaves a water row alone; `SubjectToWater` does the
  reverse; both unset or `=no` keeps the stock walk.
- `src/sim/combat/line_of_fire.rs` —
  `the_phobos_pass_walks_from_the_predicted_source`: the second pass starts at
  the given source, so a shot level with the floor from the raw coordinate
  stops on the first sample when handed a source below it.
- `src/sim/combat/line_of_fire.rs` —
  `the_flight_arm_reads_detonate_and_the_range_arm_does_not`: a
  `SubjectToLand=yes, SubjectToLand.Detonate=no` shot flies through a land cell
  on the flight arm (`isTargetingCheck = false`), still blocks on the range
  arm, and the unset companion defaults to detonating.
- `src/sim/combat/in_range.rs` —
  `only_a_spawner_foot_asks_the_source_coordinate_for_a_bridge`: the carrier
  arm holds only with a `SpawnManager`, the Foot bit and no AIR layer, and it
  probes the shot's source cell — a span one cell away does not answer.
- `src/util/flh_transform.rs` — `phobos_source_tests`: an axis-aligned target
  leaves the raw translate (Y negated), `Burst > 1` centres the lateral term,
  a +Y target turns the forward term, and the turret pivot rides the predicted
  frame (one lepton short on X, because f32 `cos(-pi/2)` is nonzero and native
  truncates).
