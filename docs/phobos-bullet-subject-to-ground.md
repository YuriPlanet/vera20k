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
- The FLH-predicted source offset (`AddFLHToSourceCoords`, `:89-125`) is not
  ported, so the ground clause still samples from the raw source coordinate.
  Native keeps `subjectToGround` only for a Foot firer that is not `IsInAir()`,
  against a target that is not `IsInAir()`, with no `Transporter` (`:92-97`);
  any miss clears the flag and returns the raw source. Otherwise it builds
  `identity → RotateZ((float)-atan2(dy, dx)) → ApplyTurretOffset` (only under
  `HasTurret()`) `→ Translate(FLH.X, Burst == 1 ? FLH.Y : 0, FLH.Z)`, then returns
  the *cell* centre (`OnBridge` → `GetCoordsWithBridge()`) plus
  `((int)x, -(int)y, (int)z)`. Two terms differ from `GetFLH`: the heading is
  predicted from the target vector rather than read from the retained facing, and
  a `Burst` above 1 centres the lateral term instead of alternating it.
- `FindFirstObstacle` samples `maxDelta + isTargetingCheck` points (`:64`), so
  `InRange` also tests the target's own cell; VERA's merged walk stops at its
  existing sample count, which has not been compared against that bound.
- After the scan, the same hook runs the spawner's position search (`:239-252`):
  a null result with `SpawnManager`, the Foot bit and `!IsInAir()` moves the
  firer's `Location` to the source coordinate, asks `0x703B10` (an elevated bridge
  nearby) and turns a true answer into the source cell as the obstacle. Neither
  `0x703B10` nor that substitution is ported.
- The bullet's own flight arm (`0x468C86` `BulletClass_ShouldExplode_Obstacles`,
  `:130-160`) calls `GetObstacle` with `isTargetingCheck = false`, where
  `.Detonate` decides between exploding and flying on. VERA's in-flight probe
  still answers only the stock cell verdict, so the four terrain keys are inert
  there (recorded, not a silent default).
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
