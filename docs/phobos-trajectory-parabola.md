# ParabolaTrajectory (`Trajectory.Parabola.*`)

Phobos' `Parabola` bullet solves a ballistic launch for one of six open-fire
modes, optionally predicts a moving target, and can bounce across the ground.
This page covers the **18 `Trajectory.Parabola.*` keys** in
`Ext/Bullet/Trajectories/ParabolaTrajectory.cpp`.

```ini
[PROJECTILE]                              ; BulletType
Trajectory=Parabola
Trajectory.Speed=100                      ; double, clamped to >= 0.001

Trajectory.Parabola.DetonationDistance=102   ; Leptons
Trajectory.Parabola.TargetSnapDistance=128   ; Leptons
Trajectory.Parabola.OpenFireMode=Speed       ; Speed/Height/Angle/
                                             ; SpeedAndHeight/HeightAndAngle/SpeedAndAngle
Trajectory.Parabola.ThrowHeight=600
Trajectory.Parabola.LaunchAngle=30
Trajectory.Parabola.LeadTimeCalculate=false
Trajectory.Parabola.DetonationAngle=-90
Trajectory.Parabola.DetonationHeight=-1
Trajectory.Parabola.BounceTimes=0
Trajectory.Parabola.BounceOnWater=false
Trajectory.Parabola.BounceDetonate=false
Trajectory.Parabola.BounceAttenuation=0.8
Trajectory.Parabola.BounceCoefficient=0.8
Trajectory.Parabola.OffsetCoord=0,0,0
Trajectory.Parabola.RotateCoord=0
Trajectory.Parabola.MirrorCoord=true
Trajectory.Parabola.UseDisperseBurst=false
Trajectory.Parabola.AxisOfRotation=0,0,1
```

## Native evidence and hooks

| Address | Hook | Behavior |
| --- | --- | --- |
| `0x468B72` | `BulletClass_Unlimbo_Trajectories` | Instance creation and `OnUnlimbo` (`:122-158`): last target, empty velocity, the `DetonationDistance <= -1` target-to-ground rewrite for a FootClass target, burst mirroring, the lead wait. |
| `0x4666F7` | `BulletClass_AI_Trajectories` | `OnAI` (`:160-175`): wait, detonation precheck, bounce, last check. |
| `0x46745C` | `BulletClass_AI_Position_Trajectories` | `OnAIVelocity` cancels the stock gravity; `BulletDetonateLastCheck` applies its own `Velocity.Z -= gravity`. |
| `0x4677D3`, `0x467927` | target-coordinate / techno checks | `SkipGameCheck`. |
| `0x4674D4` | bridge check | Native default `false` (the stock bridge arm runs). |
| `0x467E53` | pre-detonation | `OnAIPreDetonate` (`:177-201`): snap within `TargetSnapDistance`, else lift a bullet that broke through the floor. |

`PrepareForOpenFire` (`:218-311`) applies the coordinate-system angle, the
fixed `OffsetCoord` and the `Inaccurate=` scatter, then either
`CalculateBulletVelocityRightNow` (`:544-683`) or
`CalculateBulletVelocityLeadTime` (`:335-542`). The lead path solves the meet
time for `Height`/`Angle` (Newton, `:775-864`) or fixed speed
(`SolveFixedSpeedMeetTime`, `:745-773`) and falls back to the no-lead solve
when the geometry has no solution. The bounce ladder is
`CalculateBulletVelocityAfterBounce`/`GetGroundNormalVector`
(`:866-1023`); `BulletDetonateLastCheck`/`BulletDetonateEffectuate`
(`:1059-1129`) choose bounce or detonation. Detonation ends at
`DetonationAngle`, `DetonationHeight` or `DetonationDistance`.

## Where VERA20K owns it

- `rules::phobos_trajectory::ParabolaTrajectoryRules` holds every key;
  `ProjectileType::phobos_trajectory.parabola` carries them.
- `sim::world::phobos_trajectory_parabola` holds the launch solve, the lead
  Newton/analytic helpers, the bounce reflection and cliff normal, the
  per-frame checks and the target snap. The per-bullet fields are in the
  shared `PhobosTrajectoryState`.
- `BounceDetonate` detonates the bullet's own warhead and damage through the
  shared `ProjectileDetonation` boundary and does not retire the bullet
  (`TrajectoryStep::extra_detonations`). `BounceAttenuation` scales
  `payload.base_damage` afterwards, with the native `±1` floor.
- `Simulation::state_hash` folds the trajectory state.

## RNG draws

- `PrepareForOpenFire`'s `Inaccurate=` scatter draws the `RandomRanged` lane
  magnitude and then one raw direction draw through the `0x0049F420` owner
  (low byte into the retail samples). The solve, bounce and cliff probes draw
  nothing.

## Residuals (trigger · impact · frequency · risk)

- **Explode-coordinate snap hooks.** The Phobos `0x468E61`/`0x468E9F` gates on
  `SnappedToTarget` are not modelled; see `phobos-trajectory-straight.md`.
- **Retail/visual parity untested** (no retail `ini/` in this checkout).
