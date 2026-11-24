# Bullet target loss on teleport

`ifv_lifecycle_controls.py --teleport-targets` executes the original
`BulletClass::LoseTarget` (468430), `PointerExpired` (4684E0), and one subsequent
AAHeatSeeker2 guided step. Replay with the retail executable and extracted input
assets configured for the existing projectile oracle:

```sh
python -m tools.projectile_oracle.ifv_lifecycle_controls --teleport-targets --check
```

The ten supplied Unit poses exercise the marked/high-flight boundary, placement
scope, the empty cell sentinel, signed division, signed16 packing, map aliasing
and the shared dummy cell. Both callbacks execute real Unit virtual getters and
Map lookups. Original Object CRT initializers establish the height constants;
without them a marked Unit can incorrectly appear airborne in an emulator.

LoseTarget changes only the target field. It leaves the firer, cached aim,
velocity, guidance counters, proximity state and timers intact. PointerExpired
shares its coordinate decision but separately clears a matching firer. The
continuation moves the target Unit after LoseTarget and records the old Cell
passed to native HomingTrack, its velocity and candidate coordinate.

The caller is ordinary Teleport Process: after Techno detach at 7193C7,
7193CC..7193F4 scans the Bullet array backwards and calls LoseTarget for each
matching target. WarpOut, Mark(UP) and relocation follow. This scan has no Logic
membership filter and consumes no RNG. No movement, an equal/null destination,
or the active Chronosphere/state-positive path bypasses it.

Rust keeps the shared decision in `Simulation::projectile_lost_target` and the
mutation in `ProjectileStore`. A teleport resolves the unchanged target once and
applies it to matching bullets in reverse stable-ID order; repeated native
queries would stamp the same dummy coordinate. Source cleanup remains specific
to PointerExpired. The existing snapshot and hash retain the repaired target.

The fixture supplies Unit/map state and reuses the existing physical
HoverMissile/AAHeatSeeker2 reader setup; it does not run native Teleport Process,
FireAt, a whole scenario or impact damage. The Rust retail regression uses
MissileLauncher with the same BulletType and translates the supplied native
level6 coordinates to level2, preserving relative geometry and native ID/frame.
