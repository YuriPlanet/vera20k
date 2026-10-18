# Stock DISK query and laser drawing controls

`building_prism.py --disk` extends the existing `LaserScene` and `LaserPixels`
owners. The default Prism payload and command remain separate. Native output is
stored in `disk_laser.json`; its sidecar records the original image identity,
replay command and component bounds. Neither output is a whole Disk gameplay or
Display replay.

The original image SHA256 is
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.
Native body bounds below include the last byte:

| Native body | Bounds | Responsibility |
|---|---|---|
| Disk ring initialization | `4A6CF0..4A7195` | Original trig/ftol and sixteen signed XY pairs at `8A0180` |
| Techno GetFLH | `6F3AD0..6F3D5C` | Weapon FLH, actual Foot locomotor matrix, facings, transform and render coordinates |
| Jumpjet constructor / Link | `54AC40..54ACF8`, `54AD30..54AE4D` | Actual locomotor and retained owner/type parameters |
| Jumpjet / base DrawMatrix | `54DCC0..54DF45`, `55A730..55A7CE` | Zero-rocking selected matrix and original affine arithmetic |
| Object coordinates | `5F65A0..5F65C0` | Retained Location |
| Render / target coordinates | `41BE00..41BE2C`, `41BDD0..41BDFC` | Concrete Unit delegates through virtual `+48` |
| Unit high-flight delegate / Object query | `4DE620..4DE624`, `5F6B90..5F6BB1` | Marked gate and signed height threshold |
| Object GetHeight | `5F5F40..5F5F91` | Actual map floor and OnBridge correction |
| Laser constructor / DrawAll | `54FE60`, `550240` | Original registration and draw dispatch |
| General LaserDraw | `550260..5509D1` | Native offsets, fade/detail branches, Main ranged draws and software calls |
| Disk constructor / Fire / AI | `4A7A30..4A7AC0`, `4A71A0..4A733F`, `4A7340..4A78F9` | Native registration, initial angle/index and delayed emissions |
| Disk expiry caller / Detach | `725977..725994`, `4A7900..4A79CB` | Reverse callback order and actual pending append |
| Pending append / drain | `4A7FE0..4A802F`, `725C70..725D8E` | Duplicate append, readiness and common deferred compaction |
| Scalar deleting destructor | `4A7C90..4A7D13` | Inline Disk removal, base destructor and optional free |

The muzzle fixture uses actual Unit table `7F5C70` and UnitType table `7F6218`.
Unit constructor `73543A` establishes that object table; Foot constructor
`4D34DA..4D34E2` establishes the Foot bit. These object/type fields are declared
retained inputs, rather than a full Unit constructor or Scenario loader replay.
A separate Jumpjet is constructed in spare storage in the same VM and linked
through its native `ILocomotion` view at object `+4`, table `7ECD68`. Its `+24`
slot selects `54DCC0`; the separate interface table `7ECD44` is not used for
DrawMatrix. No reached getter, matrix, height, Facing or map return is supplied.

Muzzle input fields are `source`, `target`, `marked`, `on_bridge`, `seed`,
`frame`, `primary_flh`, `turret_offset`, `has_turret`, `tilt_crash_jumpjet`,
`rocking_bits`, `weapon_index`, `burst`, `base`, `primary_facing` and
`secondary_facing`. Source/target coordinates are world leptons. Both facings
are settled native Facing objects. The physical DISK post-read priors are FLH
`0,0,75`, TurretOffset `0`, Turret/TiltCrashJumpjet enabled, and zero retained
rocking. The Jumpjet parameters copied by Link are turn rate `4`, speed `16`,
climb/crash `8/15`, height `750`, acceleration `2`, wobble `.1`, no-wobbles
enabled and deviation `15`. Retail `JumpJetTurnRate`/`JumpJetAccel` differ in
case from the native keys, leaving constructor defaults `4/2`. Reader and
ordinary flight reachability evidence remain with their existing owners.

The inherited sparse flat map supplies level/slope zero at the source and
target cells. Existing Object CRT table `8141D8` and Jumpjet startup owners run
before queries. Muzzle rows retain coordinates, matrix binary32 words, actual
entry traces, independent GetHeight/high-flight traces and all three complete
RandomClass states. Marked-false skips the nested GetHeight call; `207/208`
controls reach the native threshold. Text, original tables, trig data, and
source/target/type/locomotor bodies are checked unchanged around the queries.

General draw controls execute the original constructor, then select explicit
color (`house_color=false`), stock inner `216,0,184`, outer `80,0,88`, and
zero spread. Durations, ages, fade endpoints `0/.5`, detail `0/2`, octants,
clip/camera and A/Z planes are component inputs. `fade` records actual intensity
bits at `550611`, including detail zero, where packed drawing ignores that
intensity. `general_offsets` reads the native eight rows of four signed words
at `ABC738` after GeneralDraw initializes them; Special's table is separate.

The one nonzero-spread control records each actual Main Ranged request and
return, plus complete Main/Scenario/Mapgen pre/post states seeded by the existing
native seed owner. Unmasked raw words are observed at the actual stores
`65C79D/65C84B`, including the ranged routine's inline path; function-entry
counts do not measure those draws. Nonzero outer calls Ranged three times even at zero spread;
equal bounds do not advance Raw. Existing Rally projection, BSurface memory
backing and original packed/additive DSurface bodies produce the final RGB565
surface. Laser state, A/Z planes, surface guards, original text and surface
tables are checked unchanged. No new projector, raster or RNG is introduced.

`lifecycle_priors` declares the selected weapon fields and the service boundary
IDs. `lifecycle` adds native constructor/Fire state, ordered phase snapshots,
copied LaserDraw births, actual damage/sound arguments, detach/destructor/free
observations and complete initial/final RNG state. Its source/target preparation
is the same owner used by `muzzle`. Original Disk and pending vector startup
(`4A6C70`, `725850`) run, then their real virtual Resize supplies capacity through
the existing allocator; the host does not implement insertion or compaction.

The native `55B5A1..55B5BD` caller visits Disks in reverse birth order. The
fixture stops at `55B5BE`, then executes existing LaserUpdate `550150`; it does
not run Spotlight or unrelated Logic. Frame advances before common `725C70`
drain. A birth at frame 200 first visits this rung at 201. Retained coordinate
and crash-byte changes are explicit between-rung inputs. The source stays on
the prepared flat map; these controls do not produce native movement or a crash.

The selected Range input is `1792`: physical exact-case `[DiskLaser] Range=7`
in RULESMD, absent LANGRULE and no Battle/XMP03T4 override. This is a declared
post-read input rather than another native reader. Charge and Report indices
`0/1` are fixture-relative request identities. Existing production Rules and
fixed SOUNDMD binding owners establish their retail names separately.

Damage ends at the actual `489280` call boundary with coordinates, damage,
firer, warhead, affects-tiberium flag and House recorded. No target health or
armor result is synthesized. Sound uses the inherited `7509E0` request return;
caller observations at `4A776B` and `4A76E8` preserve native request coordinates
and NULL handle before that transport changes the stack. Both sounds use the
first ring endpoint, rather than substituting owner Location or target center.
Playback and downstream receiver/audio RNG are outside this boundary.

Fire writes countdown zero and step zero. Ring emissions write countdown one;
positive visits decrement it, final discharge writes minus one, and the next
AI visit queues retirement. Current muzzle/target queries execute at emissions.
Range/crash rejection and matching expiry queue at their actual instruction.
The original expiry caller `725977..725994` executes with the declared expired
pointer. Whole Unit UnInit and other expiry-listener families are not replayed.

Pending append is not deduplicated: AI calls `4A7FE0`, while Fire/Detach contain
the same plain append inline. `725C9A..725CDF` removes every matching ticket
before one deletion. Deferred Disk deletion dispatches virtual `+20` to
`4A7C90`, which inlines the nondeleting `4A7B00..4A7B71` destructor work and
calls Abstract `4101F0`; it does not call `4A7B00`. The base body's only work
is resetting its four interface tables. Actual copied LaserDraw objects keep
their own vector/lifetime after the Disk is removed. State snapshots include
only still-registered objects, and never reinterpret freed storage as live state.
Windows SEH/readability use the existing native/IFV platform transport; original
RTTI, readiness, queue compaction and scalar destructor instructions execute.

Outer-zero `4BC750` AA, blinking, nonzero rocking, bridge/movement transitions,
whole FireAt/Logic/Scenario, damage receivers, playback and save/load remain
outside these controls. Native save/load retention has instruction evidence;
the harness does not invent stream/swizzle results. Disk's native ID `+10=-1`
and Scenario `+214` are checked throughout: its constructor calls only Abstract
`410170`, not Create_ID `410230`. Stable Rust handles do not imply a native
unique-ID cursor advance. Earlier GetROF/rearm RNG remains with FireAt's owner.

Run from the checkout with its retail environment and original image available:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.building_prism --disk --write
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.building_prism --disk --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.building_prism --check
```

Generation and the independent same-input check use the existing
`finish_vectors` owner. Run them only within the coordinated source/input
freeze; the final command checks the unchanged legacy payload and sidecar.
