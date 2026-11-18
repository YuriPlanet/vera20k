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
  or `subjectToGround` (`Ext/Bullet/Hooks.Obstacles.cpp:34,67-72`). VERA keeps
  the vanilla gate (`SubjectToCliffs || SubjectToWalls`, Ares
  `SubjectToBuildings`) and adds only the ground clause, so a projectile that
  relies on `SubjectToLand=`/`SubjectToWater=`/`Level=` entering the scan keeps
  the vanilla gate. The two keys *are* read — `PhobosSubjectToTerrain`
  (`src/rules/phobos_bullet_terrain.rs:26-59`, bound at
  `src/rules/projectile_type.rs:154`) — but nothing consumes them yet: the
  `reads_any()` half of Phobos's `SubjectToLand || SubjectToWater ? true : Level`
  decision has no caller in the `InRange` gate.
- The FLH-predicted source offset (`AddFLHToSourceCoords`, `:89-125`) is part
  of the same helper and not ported; the scan starts at the raw source
  coordinate.
- The scan feeds only `InRange`, as natively; the bullet's own flight is
  governed by the trajectory keys.

## Tests

- `src/sim/combat/line_of_fire.rs` —
  `subject_to_ground_stops_the_walk_below_the_floor`: a sample at `Z = -16` over
  flat ground blocks only when the flag is set; `Z = 0` never blocks.
