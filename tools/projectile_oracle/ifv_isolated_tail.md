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
