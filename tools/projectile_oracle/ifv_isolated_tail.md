# Original Bullet isolation and nullify tail

Six controls execute the full original Bullet detonation body `0x4690B0`.
They join the original AreaDamage receipt, animation selector, Scenario RNG,
and animation construction. The three supplied receiver states are an admitted
ordinary Unit, no receiver, and a Unit with an active Iron Curtain timer. Each
is run with `HE.EMEffect=no` and `yes`, read by the full native Warhead reader.

| Native AreaDamage result | EMEffect | Selector result | Constructed animation |
| --- | --- | --- | --- |
| 0, dispatched receiver | no | XGRYSML2 | XGRYSML2 |
| 1, no receiver | no | XGRYSML2 | XGRYSML2 |
| 2, Iron Curtain isolation | no | XGRYSML2 | IRONFX |
| 0, dispatched receiver | yes | XGRYMED1 | XGRYMED1 |
| 1, no receiver | yes | XGRYMED1 | XGRYMED1 |
| 2, Iron Curtain isolation | yes | XGRYMED1 | IRONFX |

In all three EMEffect controls, the native selector first executes
`RandomRanged(0,7)`: seed 31 produces raw word `2026076499` and index `3`.
The isolated path discards that selected ordinary animation only **after** this
RNG transition, then constructs IRONFX. Both constructor routes receive delay
0, loop 1, flags `0x2600`, native Z adjustment -15 and reverse 0. Original
Anim construction, Start, Unlimbo, position commit and Display submission run;
the constructors consume no additional RNG in these physical configurations.

Original layered `WeaponNullifyAnim` reads select IRONFX. Its full ART reader
loads the physical 4,128-byte, 48×46, 10-frame SHP from
`ra2.mix → conquer.mix`, entry `0x4ADCE3F6`; SHA256 is
`94d0dd4461d731a24ea2b2be7ca939b365e57d3d659017314dd6fff11ed9b220`.
This separate extraction leaves the earlier IFV input manifest unchanged.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONPATH=. PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/tmp/bridge-ifv-assets-1dDM9D/extract \
VERA20K_IFV_NULLIFY_ASSETS=/tmp/bridge-ifv-nullify-assets/extract \
~/Documents/vera20k-dev/.venv/bin/python \
tools/projectile_oracle/ifv_isolated_tail.py --check
```

`VERA20K_IFV_NULLIFY_ASSETS` may be omitted when `ironfx.shp` is already in the
main extracted input directory. Existing asset tooling can extract that file
without a build. No retail bytes are committed. Metadata pins the executable;
the saved result contains the physical file hashes and reader outputs.

`rows[].input` records receiver mode, authored EMEffect, live/placement XYZ,
frame, timer, health and RNG seed. `area_result` is observed from full original
AreaDamage, not supplied. `selected` and `constructed` distinguish the selector
result from the emitted effect. `constructor` records its actual arguments;
`events` preserves call/return/RNG order, including hashes at selector return
and constructor entry/return. Final native RNG hashes, animation count and
native identity cursor before/after support production comparisons.
`bullet_native_id` observes the retained Bullet's original constructor-assigned
ID 32; the partial fixture cursor is 33 before detonation and 34 afterward.

The receiver's `Unit::ReceiveDamage(0x737C90)` remains an explicit boundary:
it returns zero without mutating health. AreaDamage's original collector,
Iron Curtain timer, isolation and receipt execute around it. Native comparisons
therefore cover the enclosing receipt and animation/RNG ordering, not receiver
health or kills. Cells, occupancy and Unit state are prepared; there is no
bridge topology in these controls. Bullet's constructor CombatLight flag is
false, so the ordinary CombatLight body is not exercised. Sound binding is
absent. The test stops at Detonate return with one admitted animation and a
live Bullet; later scheduling, rendering and retirement are separate witnesses.

The Rust comparison uses production AreaDamage and Bullet effect delivery with
physical HE/IRONFX configuration. It compares the receipt, animation type and
constructor fields, and full Scenario RNG bytes. Rust executes its actual FV
receiver: at the supplied health100 boundary, damage crosses the yellow
threshold and synchronously admits `SmallGreySSys` before the impact animation.
That receiver body is excluded from this native harness. The test explicitly
asserts the one extra system, its ID34, empty child list and owner link, then
accounts for that allocation when comparing the subsequent animation and
cursor. Empty and Iron Curtain cases admit no smoke and retain direct cursor
equality. These bounds do not establish native receiver health, kills or a
whole-game identity prefix. The `EMEffect=yes` rows are explicit controls, not
a claim that retail HE enables the key.

## Stock Smashing metallic debris

The same owner has an opt-in `--brute-debris` mode and a small
[`brute_smash_debris.json`](brute_smash_debris.json) corpus. It reads physical
Smash, InvisibleLow and Smashing through the original type readers, constructs
the actual 14-name-plus-`D` MetallicDebris vector through native ReadGeneral128,
then loads the real ART/SHP inputs through the existing native reader. It does
not use the old 20-name supplied death-loop pool or VERA-exported ART scalars.

```sh
source ~/Documents/vera20k-dev/env.sh
PYTHONDONTWRITEBYTECODE=1 \
VERA20K_PROJECTILE_RENDER_ASSETS=/absolute/physical-inputs/extract \
python -m tools.projectile_oracle.ifv_isolated_tail --brute-debris --check
```

That input directory needs RULESMD, optional LANGRULE, Battle, Hills and ARTMD,
the existing IFV impact assets, the 14 real MetallicDebris SHPs, SMOKEY2,
TWLT026/036/070 and IRONFX. The existing `asset extract NAME --out DIR`
owns extraction; it writes `DIR/extract/`. No `D.SHP` is supplied: original
AnimType construction and failed ART admission retain the unread type.
Physical inputs and hashes are recorded in the payload; no retail files are
committed. Current Hills is recorded independently of the older IFV corpus.

Eight controls cover a single large/small piece, two successive Bouncer
constructors, unread `D`, native-reader zero-Min/Max, native Iron Curtain
isolation, distinct live/placement coordinates and the declared Unit receiver
boundary. Seven execute full `468D80`, full `4690B0` and the last Cluster1
successor; the coordinate control executes full `4690B0` alone. Original
AreaDamage, selector, debris budget/list selection, full Anim constructor,
Unlimbo, Logic insertion and immediate Start execute. Constructor completion
precedes the next pick. Debris receives live Bullet coordinates plus Z20,
independently of Inviso's scattered animation placement. No native gameplay
gate return is supplied to force these results.

Rows retain full Main/Scenario/MapGen RNG bytes before/after, ordered helper
requests and rejection words, independently counted raw advances, native
identity cursors, constructor arguments and returned runtime/Bounce state.
Per-constructor checkpoints retain Scenario bytes only; the unchanged Main and
MapGen streams are checked across the whole call. `input.seed` is RngTrace's
base: Main uses it, Scenario uses base+1 and MapGen base+2. Consumers restore the
retained bytes rather than infer a draw count or seed. The coverage seeds were
selected by original execution and rerun on fresh physical worlds.

All allocations succeed. Incoming admitted Bullet/source/world state is a
supplied boundary, not a full native Brute launch. Empty receiver rows execute
native AreaDamage result1; receiver/IC rows retain the existing explicitly
supplied UnitReceiveDamage return0/no health mutation while native AreaDamage
independently returns0/2. Thus native MTNK health, receiver RNG/IDs, later Anim
flight/landing and whole Logic scheduling remain outside this comparison.
The existing CRT `atexit(7C978A)` boundary returns0 without registering host
process teardown; it supplies no gameplay result. Production Brute tests and
retail-loaded release observations cover the connected actor and asset path.

## Authored typed TIRE debris

The opt-in `--typed-debris` corpus adds `[Smashing] DebrisTypes=TIRE` and
`DebrisMaximums=4` through the original Warhead reader. Stock MinDebris1/MaxDebris3
remain in force, except the explicit zero-budget control. This is an authored
scenario route using retail actors and assets. The original typed arm suppresses
SHP fallback. Eight small controls cover one/two constructors, duplicate-list
zero selection and wrap, live Bullet versus detonation placement, Iron Curtain,
zero budget, and the signed `INT_MIN` trap in the selector and four launch draws.

```sh
source ~/Documents/vera20k-dev/env.sh
# Same physical input directory as --brute-debris, plus extracted TIRE.VXL.
VERA20K_PROJECTILE_RENDER_ASSETS=/path/to/extracted-inputs \
PYTHONDONTWRITEBYTECODE=1 python -m tools.projectile_oracle.ifv_isolated_tail \
  --typed-debris --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.anim_bouncer_launch \
  --intmin --check
VERA20K_BUILDING_DEATH_ASSETS=/path/to/existing-joined-physical-inputs \
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.building_death_anims \
  --typed-debris --check
```

`typed_warhead_debris.json` preserves the Brute row's full three-stream RNG
states, ordered requests/raw words and native cursor. Per-row
`warhead_after_read` records the actual vectors/min/max; `voxels` records IDs,
null ownership, duration, full Bounce bits, deletion state and actual
VoxelAnim/Logic/AIR membership. Native startup routines initialize all three
registries, including Display arrays at `4A8630`. Constructor `7493B0` assigns
identity at `74944E` before its four raw launch draws and three Bounce requests.
The typed caller passes live coordinates without the SHP path's +20; Bounce
adds its own +10. At constructor return, the native Object coordinate still
holds the source while its Bounce position holds source+10. The Rust consumer
compares Bounce and records this transient representation bound.

Physical TIRE scalar reader `74B050` executes through its degree postpass to
`74B56C`, before VXL I/O. This is an explicit model-binding boundary, without a
supplied completed reader return or model-load success. Physical VXL identity
is retained separately. Full runtime constructor, GetCoords, identity, numeric
launch, Bounce, Unlimbo and registry publication execute. This corpus does not
establish full native Brute launch/MTNK damage, model loading, drawing, later
Voxel AI or expiry. The production encounter and renderer tests own those paths.

`anim_bouncer_launch_intmin.json` uses the existing SHP constructor owner to
exercise the same ABS-before-IDIV dependency in Z/Y/X. Its inherited map,
redraw, Display and Start boundaries remain explicit in the sidecar. The shared
`gen_rng_vectors.constrained_state` starts with original native Seed output,
changes declared independent early XOR words, then verifies them with unchanged
original `65C780`. It retains full state and constraints; no RNG function return
is replaced. Original signed wrapping absolute value of `INT_MIN` remains
negative. In the typed selector this can select no constructor and increase the
remaining budget before the next list visit.

`building_death_anims_typed_debris.json` reuses the physical joined receiver
owner and full MTNK `737C90 → 701900` fatal path. Direct damage1000, distance0,
ignoreDefenses, null attacker/sourceHouse and admitted actor state are explicit
inputs. Actual TIRE IDs4/5 precede actual ordinary `S_BRNL58` death Anim ID6;
the consumer invokes production fatal damage and never adds the adjacent Anim
by hand. This opt-in also binds MTNK's physical `DieSound=GenVehicleDie` through
original `ReadAllSoundINI7510D0` before its full type reader. Native `7021E4`
therefore consumes the Main stream before the debris Scenario draws. The row
records actual sound vectors and the original House `IsHuman50B6F0` result;
it supplies no sound playback return. No DeathWeapon is reached in this stock MTNK row. Observation ends at
receiver return, with no claim about later Logic cleanup. The original Brute,
SHP, joined-building, Kirov and Flat corpora retain their existing outputs.
