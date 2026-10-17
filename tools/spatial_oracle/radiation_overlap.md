# Stock radiation overlap arithmetic

`radiation_damage_boundary.py --overlap` extends the existing reader/map/Foot
comparison. Its default six-row corpus is unchanged. The new
`radiation_overlap.json` stores twelve short field histories and three populated
DOG receiver controls; no bulk replay or full retail files are committed.

For a native replay, set `VERA20K_RADIATION_INPUTS` to an extracted retail folder
containing `RULESMD.INI`, optional `LANGRULE.INI`, `MPBattleMD.ini` and `Hills.mmx`.
The existing populated Infantry owner also needs `VERA20K_SHRAPNEL_INPUTS` and
`VERA20K_ANYTOWN_INPUTS` as described in
[the Foot mission comparison](anytown_damage/foot_missions.md). Then run:

```sh
PYTHONPATH=. python -m tools.spatial_oracle.radiation_damage_boundary --overlap --check
PYTHONPATH=. python -m tools.spatial_oracle.radiation_damage_boundary --check
```

The sidecar records the actual executable and replay command. `layers` retains
only selected lexical inputs and source identities. Original readers execute
for RadSite Verses, Radiation fields, RadEruptionWeapon RadLevel and
RadEruptionWarhead CellSpread. The populated DOG control independently executes
its full type reader in the existing Battle/Anytown context; the selected stock
DOG/RadSite values agree with the field's Battle/Hills inputs.

The stock seven-cell separation exposes a real integer boundary. Sites centered
at `(17,24)`, `(31,24)`, `(24,17)` each contribute to `(24,24)`. Native stored sums
are `4064d55555555555`, `4074d55555555555`, `407f3fffffffffff`; the last value is
499.99999999999994. Cell487CB0 returns499 and Foot4DA604 emits99, whereas a
nearest-rounded three-way sum becomes500. The separate full Infantry517FA0 ->
Foot4D7330 -> Techno701900 -> Object5F5390 chain leaves newly constructed stock
DOGs at67,34,1 health for the33,66,99 packets. They remain alive/out of limbo;
all three receiver controls leave the tracked RNG streams unchanged.

## Execution and boundaries

Field construction uses original RadSite65B1E0, SetSpread65B4D0 and
SetRadLevel65B4F0. Activation65B580..65B74C runs the real timer/step/scalar stores;
LightSource construction and drawing are excluded, then original65B9C0 spreads
the field. Radius2688, level500, duration/remaining500 and level-steps5 are native
outputs. The inactive timer's scratch TimeLeft word is deliberately omitted.

Same-center refresh executes65B530..65B570, including actual effective-level
conversion and Decrease65BB50, before activation/spread. `tick_to90` executes the
original65B800..65B858 lifetime/level-timer prefix each frame; it reaches the
first Decay65BD00 and resets the timer. Independent light-timer and final
site-deletion suffixes are excluded.

Creation and iteration order are different: constructor65B27E..65B293 appends
to the RadSite vector; Logic55B5CD..55B5E8 visits that vector backwards before
ordinary objects. Field controls read the actual constructor-populated vector
and use that reverse order. Each stage exposes `update_order`. Reversed creation
histories retain the same sites while checking the resulting field bits. They
are explicit numeric histories, not a captured complete Logic frame.

Each stage's `samples` holds native field bits, Cell damaging level and the
original Foot factor fragment's base damage. `source_inputs` holds native-read
level/spread/timing/factor values. The field row's final Object receiver is the
old neutral300HP control; use `concrete_receivers` for the populated DOG result.
No wrapper invents an expected arithmetic value. The per-site `current_level`
executes the signed wrapping integer getter65B510 used by deployed Guard. The
separate `large_level_merge` control sets positive RadLevel50000 and exercises
65B530's float multiply/divide before adding the second50000. This prevents
reusing the wrapping getter for the merge; the two native bodies differ.
The extra `(22,29)` sample measures the five-by-five diagonal from `(17,24)`.

AbstractCtor410170, called by RadSiteCtor65B1E3, stores UniqueID=-1 at410191
and contains no calls or Scenario counter access. RadSiteCtor then appends its
own registry entry; it does not call AbstractCreateID410230. This establishes
no ID allocation at these constructors only, not a full RadSite identity or
lifecycle audit.

The two zero-step direct-kernel controls explicitly override RadLevelDelay to600
while level/duration remain500. They call Decay/Decrease directly; they do not
claim a stock site lives until that timer. Positive contributions divide by zero
and subtract to zero; outside-radius0/0 yields negative quiet NaN
`fff8000000000000`, which CellSubtract487D00 retains through its unordered
comparison. Cell487CB0 returns0 there. Only raw bits are serialized.

The native comparisons establish these numeric and receiver boundaries. Rust
production tests must separately exercise real DESO Guard/Bullet creation,
Foot application order, persistence and later cleanup. They do not establish
whole RadSite lighting, all layer/default behavior, complete native startup or
all BulletClass behavior.

## Additive light lifecycle mode

`radiation_damage_boundary.py --light-lifecycle --light-cell 33,88` uses the
existing `bridge_shadow_render.prepare(radiation=True)` / `PaletteReader`
runtime. The default six rows and `--overlap` remain separate, unchanged modes.
The additive corpus is `radiation_light_lifecycle.json`; its sidecar pins the
actual executable and replay command, with repository sources guarded only
during execution.

The component reads physical expansion `RULESMD.INI`, optional `LANGRULE.INI`,
Battle and XMP03T4 in separate native passes. Full Rules/Weapon/Warhead
constructors supply current defaults; the selected RadLevel block and complete
Radiation/Warhead readers execute. Fixed ART and destination/basic/animation/
ISO palettes use the shared archive/cache owner. Existing theater and TMP
loading choose physical filenames and construct their native descriptors.
`VERA20K_GAMEMD_EXE` selects the retail executable, as documented in
[the native runner](../native_oracle.md); the existing
`VERA20K_PROJECTILE_RENDER_ASSETS` extraction initializes `PaletteReader`.

The supplied coordinate selects an actual empty authored Cell. The decoded
physical crop within fourteen cells in each axis is provenance only. Exactly
six sample Cells are allocated: center, offsets `(1,0)`, `(0,1)`, `(5,0)`,
`(10,0)` and `(11,0)`. Their physical tile/subtile/level tuples pass original
Cell/Dummy constructors, coordinate setters and TMP slope/land queries. Sparse
fixed512 table enrollment supplies map storage; original lookup and admission
run. Full Map Resize/Recalc, authored Overlay objects, Buildings, Techno objects
and full scenario startup are outside this component. Native-read spread and
all selected physical sample/material inputs must satisfy the declared bounds.

Original LightSource still scans its full area and uses5657E0 to admit allocated
targets. Original field spread/decrease/decay instead calls5657A0 for every
target: a miss stamps the requested coordinates into the one shared Dummy and
updates its radiation double. Snapshots retain actual Dummy coordinates,
double bits, site pointer, lookup-miss/field-receiver counts and last native
field arguments, without resets or normalization. Those aliased missing-cell
values are a sparse component artifact. They do not establish ordinary complete
field state, full-circle publication order, cache/reference counts or startup ID
equivalence. Each selected Cell's gather/falloff reads its own geometry and the
source/site, without reading neighboring Cell colors or radiation. Broad field
and dispatch coverage remains the separate overlap and lamp comparisons, with
normal World integration required independently.

Two independent histories exercise a single lifetime and same-center refresh
at supplied frame45. Full activation65B580 and AI65B800 execute, including both
timers, actual LightSource registration/update, Cell483E30/484180 gathering,
Convert creation and final RadSite65B2F0/LightSource555150 deletion. Each frame
visits the current native RadSite registry in reverse order. Native observer
records retain significant timer/source/Cell states, callback order, source
expiry7258D0 and both real operator-delete call operands. Delete storage is the
existing no-op transport; the original deleting class bodies still run.

LightPublication's historical Cell sink remains the default for its old lamp
controls. This mode validates the same receiver/ABI and executes the original
Cell body, retaining only the existing Map4F42F0 redraw sink. Detail2 and final
FullInit's publication-enable store are explicit setup. Empty live listener
registries bound the full pointer-expiry calls; normal World listeners require
separate integration proof.

All three native streams are independently seeded0 after setup. Complete entry
and final states and actual request/raw-draw observations accompany the native
history. Significant live Cell/TMP rasters share that same runtime. RGB565
192x160 surfaces, point96,120, neutral A127, backgroundFFFF and oldZFFFF are
declared component priors; the actual uniform A plane is read back. The original
TMP caller/raster supplies pixels and Z. This does not prove production fog/A
upload or a complete normal frame.

The selected center's complete physical contiguous TMP family is registered:
`glat06.tem` and `glat06a.tem`, with `glat06b.tem` physically absent. Both files
use the shared descriptor constructors and lazy loader on one file transport;
their native subtile slope/land and image headers are checked. The other five
sample materials retain independent registrations. Family chain/count metadata
comes from those physical filenames; full theater filename search is outside
this component.

Each of the four phases emits one row per physical file, with explicit `phase`
and `material` fields. Count2 reaches original4814F0's fixed16 table81CCA8;
generated64 table89E620 cannot select this branch. Rotations of the actual
original fixed16 values are declared component selector inputs. Original
4814F0/547CF0 select and traverse the descriptor, observed at547F49, without
forcing a cached Cell variant or using a host modulo to select the file.
`output.selected_file_index` retains the raw native draw argument;
`material.file_index` and filename identify the descriptor actually selected.
Both selector tables and flag are restored after each phase, and all three
live RNG streams and native lighting profile must remain unchanged. Every
physical variant must show changed activation color and exact baseline color/Z
at expiry. Whole-world selector/Main/table bootstrap equivalence remains open.

Shared setup retains its historical isolated `palette_table` assertion VM and
the lexical-index transport's isolated native CRC probes. The process therefore
has more than one VM; RadSite, LightSource,
Cell gathering and selected live TMP raster all execute in the one existing
central runtime. No radiation formula or expected color is supplied by Python.
Actual paid DESO creation, Guard/deploy/shot scheduling, receiver damage and
audio remain independent production integration obligations.

After the coordinated input/source freeze, the gated commands are:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.radiation_damage_boundary --light-lifecycle --light-cell 33,88 --write
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.radiation_damage_boundary --light-lifecycle --light-cell 33,88 --check
```

The coordinate is a reusable explicit input, not a forced live-game location.
The generator fails closed on missing physical geometry/materials, unsupported
spread, unfinished native calls, absent light-to-pixels change, incomplete
cleanup, timer-bound exhaustion, input drift or unexpected RNG activity. Source
authoring alone does not establish that these gates have passed.

The shared Reader invocation default remains2M instructions/10s. Only this mode's
full outer activation65B580, refresh65B530 and AI65B800 opt into20M/120s through
that same owner, because native area publication can construct many27/53-shade
Cell Convert tables. The original return boundary is still required; exceeding
either limit fails. These bounds alter no native inputs, instructions or results.
The mode's source observer filters irrelevant PCs before reading registers;
its return-PC index is rebuilt from `NativeCallTrace.pending` after each trace
mutation, preserving that owner's nested ESP checks. Publication hooks observe
only the two existing Cell/Map sink addresses. Native palette instructions and
Cell gathering still execute in full.

Full getter entry/return tracing remains active during execution and must leave
no pending native return. After those checks, output serialization omits the
repeated field `cell_coords` receipts and maps every stage event range to its
exact filtered slice. The small consumed corpus retains all other lifecycle
events and every state, timer, Dummy, RNG, material and pixel observation.
Unconsumed bulk getter logs are not committed as a raw archive.

After both histories have actually retired, the last central runtime also runs
three numeric controls through original `65B871..65B89E`: ordinary positive
operands, low32 multiplication overflow, and signed operands. Explicit zeroed
operand storage supplies base tint, remaining time and duration; it is not a
constructed RadSite. The original three IMUL/IDIV pairs and argument pushes
execute within200 instructions. The corpus records their actual visited PCs and
pushed red/green/blue values, stopping before the LightSource call. No Python
formula supplies expected channels; the signed prior is not an ordinary negative
field claim.

Two further controls execute full `554AA0` with supplied current/proposed
parameters in inactive zeroed numeric storage, without a LightSource constructor.
One changes green while retaining intensity, red and blue; the other changes
intensity. The original comparison gate and parameter stores execute within200
instructions, and actual before/after fields are recorded. Active0 must prevent
area publication. Both kinds of numeric control save and restore CPU/stack and
assert unchanged live registries, native runtime-ID cursor, final Cell/source
state, callback counts and all three RNG streams. They add small separate
top-level control arrays without altering the two histories or their surfaces.

## Snow light lifecycle

The additive `--snow-light-lifecycle` mode uses archived `multimd.mix/XXmas.MAP`
(entryA813FA32), default center37,86, and writes the separate
`radiation_snow_light_lifecycle.json`/`.meta.json`. The existing TEMP command and
corpus are unchanged. The same two histories, sparse six actual physical Cells,
source/timer owners, five numeric controls and central runtime are reused.

Before the Cell palette setup, the original Map/Theater reader
687631..68764F→475870→48DBE0 stores Scenario+1258=SNOW1. Native theater record
7E1BE8 and its filename formats bind SNOWMD.INI, `.sno`, ISOSNO.PAL and SNOW.PAL.
The existing original palette expansion/default CellConvert owner consumes the
physical Snow source/destination bytes. The original SNOWMD General/TileSet
reader and XXmas Lighting reader run; expansionRULESMD, optional LANGRULE,
Battle and XXmas re-read the selected radiation/weapon/warheads in that order.
No held Reader or NativeAudioPlatform implementation is replaced.

Full65B580 reads the actual native theater field. Its Snow1 branch
65B5F2..65B602 sets working red and blue to signedFFFFFF80 while retaining the
Rules green byte. The signed integer normalization sequences65B604..65B684,
tint-factor double multiplication, upper1000 bound and original ftol execute;
no lower-zero clamp or guessed channel result is supplied. The corpus records
actual working channel bits at65B604 before those conversions and retains
actual produced source/Cell/RGB565 values. Same-center refresh remains supplied
before AI45; remaining lifetime and independent light timer, native registry
order, full three RNG states/requests, two-receiver7258D0 expiry and actual source
delete operands are recorded through the shared lifecycle owner.

The selected physical tile0 family is all eight `clear01.sno`, `a` through`g`
files from `ra2.mix/isosnow.mix`; `clear01h.sno` is absent. Actual native
descriptor construction, lazy loading, metadata and chain/count operate on one
transport. Count8 uses original4814F0's Main-generated64 table. The same value
rotations0..7 used by the existing Clear oracle are explicit component inputs;
original selection and547CF0 traversal determine the observed effective file.
Each phase requires all eight physical descriptors, preserving raw versus
effective identity and restoring both tables/flag/profile/live RNG. Every file
must show changed activation pixels and exact baseline color/Z at expiry.
These controls do not certify full World Main/table startup or native map
construction. Normal paid DESO/command/audio/fog/GPU integration is separate.

After source/input freeze and the explicit native execution gate:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.radiation_damage_boundary --snow-light-lifecycle --light-cell 37,86 --write
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.radiation_damage_boundary --snow-light-lifecycle --light-cell 37,86 --check
```

Source authoring does not establish these native results. The existing TEMP
`--light-lifecycle --check`, Clear `bridge_shadow_render --tile --check`, and
default selected-tile/low-bridge comparison protect the affected historical
branches; execute them under the same coordinated source guards.
