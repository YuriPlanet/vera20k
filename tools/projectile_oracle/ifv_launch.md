# FV guided bridge-shot evidence ledger

Current chain: ordinary empty FV → HoverMissile Burst=2 → AAHeatSeeker2/HE →
a static high bridge, with DRAGON body/trail and retirement. This page indexes
the bounded native comparisons and current Rust validation; neither this chain
nor the wider whole-bridge goal is declared complete. Updated 2026-09-26.

## Launch corpus and boundary

`ifv_launch.py/json/meta.json` preserve the five-case research payload without
schema or semantic changes. Metadata records physical file hashes, Unicorn version and explicit boundaries.
The common preparation is imported from `guided_step.py`. No `/tmp` import is
required.

Configure the original executable using [native_oracle.md](../native_oracle.md)
and the extracted input directory used by `bridge_render_inputs`, then run:

```sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-inputs \
python -m tools.projectile_oracle.ifv_launch --check
```

The original source empty-gunner slice70DC70 and selection6F3330/70E140 choose
slot0 HoverMissile from physical FV rules. Full Weapon/Bullet readers and
Weapon Process/postpass establish the guided launch inputs. The physical HE
reader also runs. Actual FireAt6FE4F2 through launch6FF01A executes the original
Bullet COM factory, constructor and launch math. Its local cursor28 assigns
Bullet29. Windows InterlockedIncrement, ObjectUnlimbo and Display admission are
supplied boundaries. This partial prefix is not a whole-scenario identity claim.

The fixture supplies source Unit lifecycle, position, facing and FLH rather than
executing their producers. Its type is a base TechnoType allocation: it must not
be reused for UnitType+E48 GetROF/BurstDelay reads. The independent
[numbered FLH and Burst fixture](ifv_fire_coord.md) uses a real UnitType and
executes those bounded prerequisites.

After launch, whole Bullet AI4666E0 executes until original AreaDamage489280
entry. Five cases record125 total visits: direct live bridge contact, no bridge,
ahead bridge-band interception, a host-cleared band and a host-cleared/restored
band. Original Map/Cell floor, homing, step phase, bridge crossing, detector and
impact handoff execute on mapped level6 flat cells. Host writes structural
bit100 changes between specified frames; native bridge collapse/repair drivers
are not simulated by those writes. Rows preserve XYZ, velocity bits, course and
closing state, visited calls, detector timers and impact arguments.

The frozen research JSON and promoted JSON were compared for full semantic
equality; independent `--check` reproduced both this payload and the separate
FLH/Burst corpus on2026-09-26. The production flight comparison now passes all 125
rows (receipt below).
These results do not certify Unit AI/shot scheduling, the post-launch FireAt
tail, AreaDamage effects, bridge collapse/repair, rendering or object retirement.
The separate impact/lifetime work follows those dependencies; its receipt must
be cited when claiming them.


## Current mechanism evidence

All native corpora use the executable SHA above. A native comparison applies
only to its declared inputs and boundary; supplied lifecycle, map and scene
state does not establish the unexecuted producers.

| Mechanism and authoritative owner | Native evidence | Current Rust evidence |
| --- | --- | --- |
| Retained Speed/binding and prior-pass Gravity; `rules/native_processing` and shared launch speed | [weapon_speed](../rules_oracle/weapon_speed.py), [weapon_speed_order](../rules_oracle/weapon_speed_order.py): original constructors, readers and complete Process/postpass, including 28 sequential pass rows | Speed prerequisite d7b37a3d: native-processing 58, weapon 10, launch 2 pass; the 5 table-dependent launch checks were separately run by root. Production retail cold/scenario passes are included. |
| General MissileROTVar/MissileSafetyAltitude; retained registry receipt → RuleSet | [guided_controls](../rules_oracle/guided_controls.md): full Rules ctor, 30 sequential scalar reads; defaults 0.25/500, exact key MissileSafetyAltitude, `%f` scan before widening to f64 | `guided_controls_tests` passes 30 altitude and 27 float comparisons. Failed-scan/successor and minimum-subnormal float rows are explicit exclusions. |
| Native runtime IDs; `Simulation.native_unique_ids` | Original Abstract constructor410230/AssignUniqueID68BCB0 and [native_id_snapshot](../spatial_oracle/native_id_snapshot.py); local Bullet/Anim receipts in the linked launch, Burst and impact corpora | Strict shared cursor, wrapping signed IDs separate from stable handles. Techno/Bullet/Anim retain receipts; deferred preconsumed death-Anim publication does not allocate again. Identity 10, constructor 30 and Anim 45 passed earlier; the joined candidate also passes snapshot 94 and hash 83. |
| Numbered FLH and flat Drive pose; `ArtEntry::weapon_flh` → `combat/fire_coord` | [ifv_fire_coord](ifv_fire_coord.md): eight ART controls and original GetFLH/Drive transforms; native weapon index and null-elite fallback | FLH 9 pass. Physical FV produces muzzles `[2751,5200,980]` and `[2752,5295,980]` from supplied `[2688,5248,800]`, body 0/turret 0x3fff. |
| Two-shot FireAt emission; existing `world_fire::emit_admitted_fire` | [ifv_fire_coord](ifv_fire_coord.md): real UnitType, original FireAt factory/ctor and rearm tail; cursor 21 → Bullet 22 → Bullet 23; frame 0/duration 4 then frame 4/duration 50 | Production two-shot comparison passes positions, IDs, Burst 0→1→0, timers and complete Scenario RNG buffers. Supplied post-admission boundary; physical HoverMissile Range6/MinimumRange1 (the earlier Range5 description was incorrect). Ordinary admission and Unit scheduling are not claimed. |
| Guided flight; `ProjectileStore` plus `projectile/homing` | [ifv_launch](ifv_launch.py), [guided_step](guided_step.py), [ifv_guided_controls](ifv_guided_controls.md): whole 125 AI visits,19 step/phase rows, 38 ramp visits and 31 boundaries | Root reports all 4 homing tests pass: launch plus `advance_one`, velocity bits, coordinates, course/closing/lock state and host-controlled live bridge-bit changes. Current state replaces approximate heading/age/phase with native velocity and counters. |
| Impact selection and effect publication; shared `combat/detonation_anim` and native rules receipt | [ifv_select_anim](ifv_select_anim.md), [retained inputs/reset](../rules_oracle/select_anim_inputs.md): full selector controls; actual Conventional/EMEffect/AnimList, Lightning/Weather/Nullify and CombatDamage SplashList keys | Joined candidate passes 48 selector controls, 5 dummy-land controls, 22 AreaDamage receipt controls and 2 rules tests covering 12 retained passes plus reset/reread. [Six original isolated-tail controls](ifv_isolated_tail.md) independently reproduce selection-before-nullify RNG order; the final focused candidate passes those 6 Rust controls plus all 5 native first-ground-occupant controls and the stated list invariants. |
| Physical impact and cleanup | [ifv_impact](ifv_impact.md), [ifv_bridge_impact](ifv_bridge_impact.md), [ifv_trail_impact](ifv_trail_impact.md): original AreaDamage/bridge mutation, Anim constructors, Bullet retirement; baseline XGRYSML2 completes 14 AnimAI visits | Joined ordinary retail IFV world test passes flight, collapse, target release, trail detach and restore continuation. The final focused candidate passes the composed native three-hit comparison, including original body priming 9→15 with return 0 and all 225 retained cell states. |
| DRAGON body/trail | [ifv_render](ifv_render.md): 166 physical native draws; [line_trail](line_trail.md): readers, cadence, ring, pixels, overlap and load behavior | Joined DRAGON comparison passes 166 CPU frame/projection rows and 166 GPU pixel rows in each of BGRA/RGBA (1.20 s). Earlier trail CPU group 6, 42 GPU cases × 2 formats and 5 overlap rows × 2 formats pass; only the 8-trail/A127 row was additionally checked with forced 3-operation chunks. Whole-scene rendering is not certified. |

For Burst RNG, the first GetROF draw in 3..5 rejects raw 2026076499, then takes
2287577493→4. At frame 4 the next raw 2225548056 yields 0 in 0..2 and the retained
ROF becomes 50. Native constructor receipts precede each shot's later effect and
rearm work. These exact counts/order apply to the recorded supplied prefix,
not to every weapon or whole-match constructor sequence.

## Persistence and validation receipts

The merged FV launch candidate uses snapshot 217, `HashFeature::NativeRuntimeIdentity` 217 and
simulation-config namespace 9. The cursor (including saved prefix/phase), retained
runtime IDs and guided velocity/course/closing state affect future behavior and
are hashed/persisted. Config identity includes retained Speed/projectile binding,
Gravity, General guidance controls, selected ART FLH/trail data and selector
references/lists. Missing production cursor is an initialization error in every
build; only explicit synthetic simulation construction installs a synthetic
cursor. Native constructor identities never substitute for stable object handles.

The joined candidate is retained locally as `bridge-ifv-joined-libtests`, SHA256
`40f4b5d6d42b5f56da44f8b495d5f083c86dc4ab04594760abebb420810a7970`.
Its focused checks pass AreaDamage 22, selector 48, dummy-land 5, the 2 rules tests
(12 retained passes and reset), two-shot FireAt, DRAGON 166 CPU rows, and
DRAGON 166 GPU rows in each of BGRA/RGBA. Related module checks pass Bomb 17,
world-hash 83, snapshot 94 and ART 76 (2 ignored). These are counts of controls or
module tests as named, not one combined full-suite count.

On that candidate,
`retail_ifv_forcefire_guidance_survives_restore_and_collapses_live_bridge` passes
in 108.59 s: 196 shots reach collapse at frame 5348; a saved live Bullet continues
and retires after 20 ordinary frames. Two collapse restores match all 200 future
state hashes through the retained Bouncer's flight and expiry, ending at
`a9efda03f15dbb00`. The witness also checks bridge surfaces/navigation, target
release and saved Display membership. This is production Rust integration and
deterministic continuation, not a native whole-match trajectory comparison.
The local receipt is `/tmp/bridge-ifv-joined-live.log`. Trail rings remain
presentation state: original ObjectLoad clears the owner pointer and production
load clears history rather than serializing rings into simulation snapshots.

The earlier [trail receipt](line_trail.md) retains its separate binary and timing
bounds. The five native overlap rows pass at normal capacity in both formats;
only the 8-trail/A127 control also passes forced 3-operation chunks (80 chunks,
160 passes, crossing the bounded submission boundary). This is not a claim that
all overlap rows or full 32-sample rings were tested at every chunk size.

After the fixture and assertion corrections, the final focused candidate
`bridge-ifv-final-focused-libtests`, SHA256
`e12b7834225b63dcef6b0f6c8d5b596a9bcfc0b1cb3c930bdcfefd32bc76ef20`,
passes all 6 isolated-tail controls (1.21 s), all 3 composed bridge impacts
(0.31 s), the 5 native naval controls plus list/deck/height invariants (0.70 s),
all 151 combat tests and all 13 Lightning tests. The corrected fixtures publish
registered AnimTypes through the production registry owner and observe actual
inline constructors. They do not reconstruct deferred events or fabricate
loaded frame metadata. Original isolated-tail `--check` also independently
passes all 6 controls.

The final retail library suite passes **9575 tests, 0 failures, 164 ignored**
(59.47 s test execution; 80.97 s including rebuild). `cargo clippy -p vera20k
--lib` exits 0 in 15.98 s with warnings; this is not a warning-free claim.
Both ran with `VERA20K_REQUIRE_RETAIL_INI=1`. The retained final test binary is
`bridge-ifv-repaired-final-libtests`, SHA256
`3f321fa4690d47dd0489c8073808d5ada0fa2e7e0b8dd9feb2d41d0c006f113b`;
local logs are `/tmp/bridge-ifv-repaired-final-{full-lib,clippy}.log`.

An earlier full run found ten failures. Repairs included hashing absent and
explicitly zero FLH as the same effective configuration; observing actual
inline Anim constructors in the raised-ground fixture; explicit synthetic ID
setup; and [native-executed lifecycle fixture controls](ifv_lifecycle_controls.md).
Before updating current schema-217 pins, all three replay fixtures reproduced
their prior hashes with `Before(217)` and passed every earlier projection. The
old current pins remain executable assertions; only the new identity/fallback
Land folds change these fixtures' current hashes. These are Rust replay
regressions, not native whole-match evidence.

The release application and example build passed in 55.08 s. Retained app
SHA256 is `7878eca1954b3a9fba455e222bbabdb298b47dc82a87ce6fe7b17b868e158758`;
example is `a2a448357175fe5b2a898ffc0904328fc7e4fc59d7ff2f5940245f42da9dca5a`.
The release production witness passed in 10.58 s on loose
`bridge-debris-hills-20260926.yrm`, byte-equal to entry `d5fe80ac` inside retail
`Hills.mmx`; payload SHA256 is
`780d5d6e6d3c81ac0d510a5df326848dd426b3d840ac290114f44faf8eab9e9e`.
It saved live Bullet962 at `[16464,18492,1220]`, collapsed the selected bridge
after196 missiles at frame5348, then advanced200 frames through DBRIS3LG flight
and expiry with TWLT036 follow-up. Final hash `4229dfd2711ecdc0` includes the
explicit game-speed6 command and loose map identity; it is not directly the
ordinary test fixture's hash. Receipt: `/tmp/bridge-ifv-release/retail-map-load.json`.

The same ordinary release app was launched at the normal menu with the primary
checkout as cwd and only RA2_DIR retained among RA2_* variables. Single Player
→ Skirmish → the loose retail map reached a responsive game. Ordinary pause-menu
load restored the fresh flight save, collapse save and flight save again. Visible
captures show intact deck → broken middle span → intact deck. They establish
production restoration and rendering, not individual missile pixels or native
whole-scene parity. The validation session was closed afterward. Captures:

- `SCRN0014.pcx`: SHA256 `f96637d17a3be0489a8b5956956f04872b41cc8bc3b12b534dee0d5d3a80b365`.
- `SCRN0015.pcx`: SHA256 `8365774a2ed4d7f3f10ca0d1d31a27355b37e94cc19f36b30231f107fb0a73d0`.
- `SCRN0016.pcx`: SHA256 `0738910240475089c35091cda89b9bce7a24990ca1dcd49c0baf467f13a73f85`.

PCXs are in the primary checkout; their 1024×768 PNG conversions, launch and
capture receipts are under `/tmp/bridge-ifv-release/`.
After this release build, warning triage removed only the unused copied
`ProjectedLine.color` field, its assignment and its test initializer. Authoritative
segment color and GPU extraction remain unchanged. Cleanup test binary SHA256
`345daa89e1c14df1955aeee3d49aa7443f0c658142da70fc46747100189fe75b` passes all5
trail CPU tests and both production GPU tests:42 pixel controls and5 overlap
rows, both formats, plus the one declared forced-chunk control. Logs are
`/tmp/bridge-ifv-cleanup-{cpu,gpu}.log`.
The post-cleanup final release rebuild passes in51.95s, with app SHA256
`ad407cd801eadc5b3f8af58584a124d175288e719c4bc0e065cdc33ff01476d0` and example
`7bf9e4050c37a9fe417304847401fc7c41df44c24ec45ed2556ae2b4274f0980`.
Its retail production witness passes again in10.48s with the same196 shots,
frame5348 collapse and final `4229dfd2711ecdc0`; fresh save hashes and paths are
in `/tmp/bridge-ifv-final-release/retail-map-load.json`. No simulation or
rendering behavior changed in this three-line cleanup. The single fresh critic is complete (below); PR publication remains pending.
No whole-bridge completion is claimed.

## Single fresh critic disposition

The read-only critic reviewed `f63ec3833cb5ce4892eb259de389d6e4e0faa70f`
against `350ae69591928d0054b70ce42acda28b5121f824`, including the Speed
prerequisite and subsequent documentation receipt corrections. It found no
confirmed actionable implementation defect or blocking change request in the
selected empty-FV/static-high-bridge chain, and no consequential additional
in-scope refactor. It independently passed15 retained Rust tests covering
homing, two-shot FireAt, AreaDamage/nullify, composed bridge impact and trail
CPU/GPU behavior, plus the original31-control guidance `--check`. It inspected
the full-suite, Clippy and final release receipts without rerunning them.

The reviewer's initial homing invocation pointed to the wrong retail directory
and therefore used fallback tables; correcting the path to the configured
retail inputs made all4 comparisons pass. The report records that invocation
error rather than attributing it to the candidate. The single report is retained
locally at `/tmp/bridge-ifv-critic.md`. Its full-world ID-prefix, Unit scheduling,
receiver, other-consumer, whole-scene/platform and scaling limitations remain
explicit in this ledger. This pass does not close the whole-bridge goal.

## Local Ghidra persistence

Relevant comments were saved and read back in the local `gamemd.exe` program:
HomingTrack `0x5B20F0`; LineTrail visit/sample/detach `0x556D40`, `0x556B30`,
`0x556C00`; retained weapon Speed `0x7729F0`; SelectAnim `0x48A4F0`;
AreaDamage `0x489280`; and Bullet detonation `0x4690B0`. The global at
`0xA8EB78` is corrected to `g_nOptionsDetailLevel` (integer). The final Bullet
comment records the six executed Area0/1/2 controls, selection-before-nullify
RNG, constructor ordering and supplied receiver boundary. Local receipts are
`/tmp/bridge-ifv-{guided,speed,selector,area-tail,isolated-tail}-ghidra.json`.
These database edits are separate from Git; the tracked harnesses and this
ledger preserve the reproducible evidence.

## Open boundaries

- Native Type reset nulls LightningWarhead but leaves deleted Anim addresses in
  WeatherConBoltExplosion, WeaponNullifyAnim and SplashList. Rust explicitly
  clears these unsafe references. The reset corpus proves selected physical
  reread through the first complete Process call, not later file reloads or
  native dangling-pointer reuse; there is no rebinding by name.
- Broader DeathWeapon still uses the existing bare AreaDamage route rather than
  the original temporary Bullet factory. The empty-bridge fixture does not
  exercise it. Shared Bomb/Rocket/Lightning selector migration does not certify
  their whole lifecycles; shrapnel child RNG/interleaving and other projectile
  consumers need their own complete-chain evidence.
- Particle/Wave/VoxelAnim and unexecuted Smudge/nested constructor branches, plus
  the full world startup pool/order, are outside this runtime-ID proof. Local
  cursor receipts must not be described as globally correct match IDs.
- The selected source transform is flat Drive. Slopes and arbitrary-coordinate
  precision, failed/repeated Unlimbo and complete Unit/Display scheduling remain
  bounded separately. Malformed coordinate/RGB scans and malformed/subnormal
  float reads have documented deterministic policies, not fabricated native
  defaults.
- Trail GPU pixels are exact for the supplied Z/ABuffer/background cases. The
  shared fog-edge background uses an approximate sRGB curve and the app's late
  global shroud composition; later native RadBeam/other effect overlap is not
  established. Zoom 1 is the native pixel domain. Timing covers debug 64×96
  fixtures, not release FPS or 20,000-unit scenes. These limits preclude a
  whole-scene or whole-bridge parity claim.
