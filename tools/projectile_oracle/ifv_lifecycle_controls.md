# Original guided controls for lifecycle test fixtures

Three controls execute original Bullet AI `0x4668BD..0x467B7A` and reached
`HomingTrack(0x5B20F0)` using the existing [guided-control harness](ifv_guided_controls.md).
They establish the incoming states used by two focused lifecycle regressions;
they do not execute pointer expiry, snapshots, scheduling, damage or retirement.
The physical input hashes and supplied boundaries are
recorded in the JSON and companion metadata. Independent `--check` passes.

```sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-ifv-inputs \
python -m tools.projectile_oracle.ifv_lifecycle_controls --check
```

Configure the native executable as described in [native_oracle.md](../native_oracle.md).
Only tracked helpers are imported. Generation changes the supplied Python
state table; no original instruction or callee is replaced. `--write` regenerates
both the result and provenance, while `--check` does not write.

| Original control | Supplied input | Original output before coordinate commit |
| --- | --- | --- |
| `ground_old1_level_no_acceleration` | Level=yes, Acceleration=0, maximum4, velocity `[4,0,-2]`, old height1 | Candidate height−1, impact0 |
| `ground_old0_level_no_acceleration` | Same inputs, old height0 | Candidate height−2, impact1 |
| `expired_null_below_safety` | Null target, maximum0, velocity `[64,0,0]`, initial lock=true, old height400, safety500 | No impact; velocity bits `[4046461be6800000,0000000000000000,4046461be6800000]` |

These results explain two obsolete fixture assumptions. Ordinary non-Level
homing can add 18 leptons of cruise clearance, so a supplied downward velocity
does not itself guarantee a ground crossing. The ground controls explicitly
suppress that correction and acceleration to isolate the old-height predicate.
A null target uses the separate pitch-up arm, preserving yaw; it does not aim
at the origin. Moreover, original `0x466E70` detonates a null-target missile when
its old height reaches MissileSafetyAltitude, as the existing 499/500/501
[controls](ifv_guided_controls.md) demonstrate.

`homing_ground_impact_reaches_damage_and_cleanup_through_runtime_frame` consumes
the first native candidate height and retains its real wall-damage, live/expired
JumpJet source, fuse and retirement assertions. The Rust fixture translates
native level6 to level2 and cells `[10,20]`/`[17,20]` to `[5,5]`/`[12,5]`.
The comparison bounds this aligned first crossing, not a retail AAHeatSeeker2
flight with authored Level/Acceleration defaults.

`gsi_05_04_high_flying_source_and_target_become_explicit_null` retains the actual
pointer-expiry and save/load assertions, stays at height400 below safety, then
compares all three native velocity words after restored AI. Its native ID100
and binary frame1 match the original control. It translates native floor624 to0
and uses a different absolute XY; the null pitch-arm velocity is comparable,
while the coordinate-dependent closing accumulator is deliberately excluded.
The native control does not prove the fixture's pointer-expiry/save producers.
The repaired candidate passes all 149 `sim::world::lifecycle_tests` checks,
including both corrected fixtures. The retained binary SHA256 is
`de60d5197d60bb144ec0bc5fd350a954b5154a70594c447d18594d89b1a7e1bf`;
the local receipt is `/tmp/bridge-ifv-repair-lifecycle.log`. This Rust integration
receipt does not extend the original three controls to unexecuted native
pointer-expiry, save/load, damage or scheduling producers.
