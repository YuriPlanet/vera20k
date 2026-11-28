# BombardTrajectory (`Trajectory.Bombard.*`)

Phobos' `Bombard` bullet lobs to a turning point above the target and then
falls. This page covers the **21 `Trajectory.Bombard.*` keys** and the
per-bullet arms in `Ext/Bullet/Trajectories/BombardTrajectory.cpp`.

```ini
[PROJECTILE]                              ; BulletType
Trajectory=Bombard
Trajectory.Speed=100                      ; double, clamped to >= 0.001

Trajectory.Bombard.Height=0                     ; double leptons, clamped >= 0
Trajectory.Bombard.FallPercent=1
Trajectory.Bombard.FallPercentShift=0
Trajectory.Bombard.FallScatter.Min=0            ; Leptons
Trajectory.Bombard.FallScatter.Max=0            ; Leptons
Trajectory.Bombard.FallScatter.Linear=false
Trajectory.Bombard.FallSpeed=0                  ; 0 -> Trajectory.Speed
Trajectory.Bombard.DetonationDistance=102       ; Leptons
Trajectory.Bombard.DetonationHeight=-1
Trajectory.Bombard.EarlyDetonation=false
Trajectory.Bombard.TargetSnapDistance=128       ; Leptons
Trajectory.Bombard.FreeFallOnTarget=true
Trajectory.Bombard.LeadTimeCalculate=false
Trajectory.Bombard.NoLaunch=false
Trajectory.Bombard.TurningPointAnims=
Trajectory.Bombard.OffsetCoord=0,0,0
Trajectory.Bombard.RotateCoord=0
Trajectory.Bombard.MirrorCoord=true
Trajectory.Bombard.UseDisperseBurst=false
Trajectory.Bombard.AxisOfRotation=0,0,1
Trajectory.Bombard.SubjectToGround=false
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x468B72` | `BulletClass_Unlimbo_Trajectories` | Creates the instance and runs `OnUnlimbo`: `Height += TargetCoords.Z`, the `RandomRanged(0, 200 * FallPercentShift)` draw, the initial/last target record, empty velocity, burst mirroring, the lead wait (`:115-144`). |
| `0x4666F7` | `BulletClass_AI_Trajectories` | `OnAI` (`:146-161`): wait, detonation precheck, velocity change, falling-remain check. |
| `0x46745C` | `BulletClass_AI_Position_Trajectories` | `OnAIVelocity` adds the type gravity back, so the stock tail has no net gravity; `FreeFallOnTarget` applies its own `Velocity.Z -= gravity` (`:177-180`, `:583-586`). |
| `0x4677D3`, `0x467927` | target-coordinate / techno checks | `SkipGameCheck`. |
| `0x4674D4` | bridge check | `ShouldSkipBridgeCheck()` = `!SubjectToGround`. |
| `0x467E53` | pre-detonation | `OnAIPreDetonate` snaps to the target within `TargetSnapDistance` (`:163-175`). |

`PrepareForOpenFire` (`:192-245`) first resolves the target coordinate
(`CalculateTargetCoords`, `:283-323`: optional `NoLaunch` lead,
coordinate-system angle, `OffsetCoord`, `Inaccurate=` scatter), then either
launches toward `CalculateMiddleCoords` at `Trajectory.Speed`, or starts the
`NoLaunch` fall at `FallSpeed`. `BulletVelocityChange` (`:514-587`) decrements
`RemainingDistance`, marks `ToFalling`, normalizes the fall velocity, and
consumes the final `RemainingDistance` in `BulletDetonateRemainCheck`
(`:497-512`). `CalculateBulletLeadTime` (`:325-406`) is the three-arm solve
for `FreeFallOnTarget`, `NoLaunch` and the straight fall speed.

## Where VERA20K owns it

- `rules::phobos_trajectory::BombardTrajectoryRules` holds every key;
  `ProjectileType::phobos_trajectory.bombard` carries them.
- `sim::world::phobos_trajectory_bombard` holds `OnUnlimbo`,
  `PrepareForOpenFire`, the lead solve, the middle-coordinate scatter, the
  per-frame velocity change and the target snap. The per-bullet fields are in
  the shared `PhobosTrajectoryState`.
- Movement uses `ProjectileCollisionWorld::phobos_trajectory_motion`; the
  final detonation uses the shared impact ladder and `ProjectileDetonation`
  boundary. `Simulation::state_hash` folds the trajectory state.
- `Bounce`-style extra warheads and `PassDetonate`/proximity hits ride
  `TrajectoryStep::extra_detonations`, which do not retire the bullet.

## RNG draws

1. Unlimbo: `RandomRanged(0, int(200 * FallPercentShift))`.
2. Middle coordinates: `RandomRanged(FallScatter.Min, Max)` and then one
   `RandomDouble` (radial) or one `RandomRanged(0, 1)` (linear).
3. `Inaccurate=` scatter: `RandomRanged` handled above, then one raw direction
   draw through the `0x0049F420` owner (low byte into the retail samples).

`TurningPointAnims` picks one resolvable name with the native
`RandomRanged(0, size - 1)` draw and constructs it at the turning point
through `spawn_random_owned_anim` with the firer House (or the retained
`BulletExt::FirerHouse`) and no owner object (`:241-243`, `:565-569`).

## Residuals (trigger · impact · frequency · risk)

- **Bullet `LaserTrails` reset.** The native `LaserTrails` `LastLocation`
  reset is the presentation trail owner's; VERA models the stock `UseLineTrail`
  line trail, not bullet laser trails. Frequency: data with trails. Risk:
  visual.
- **Explode-coordinate snap hooks.** The Phobos `0x468E61`/`0x468E9F` gates on
  `SnappedToTarget` are not modelled; see `phobos-trajectory-straight.md`.
- **Bridge check.** `ShouldSkipBridgeCheck`'s `SubjectToGround` input has no
  field in the motion call, so a falling bomb still takes the stock bridge
  arm; its turning-point animation now stores the bullet owner as the invoker,
  matching `CreateRandomAnim(..., pBullet->Owner, pOwner, true)`.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
