# Populated Building-slot translucency

From the checkout, with the original game archives beside the configured
executable and `tools/requirements-test.txt` installed:

```powershell
$env:VERA20K_GAMEMD_EXE = 'C:/your-retail-install/gamemd.exe'
python -m tools.spatial_oracle.building_anim_translucency --check
```

The default checks without writes. `--write` deliberately regenerates the
adjacent JSON and provenance sidecar for review. Import and `--help` perform
no native execution or asset reads. The producer uses `finish_vectors` with
the selected setup/transport owners guarded before and after generation.
Neither executable identities nor repository source hashes are published.
Physical asset digests identify inputs; their bytes are not embedded.
The complete unprojected receipt is retained in `building_anim_translucency.raw.json.gz`
with its own provenance sidecar. `finish_vectors` writes deterministic gzip
(level 9, zero timestamp, empty filename); `--check` compares both the projected
JSON and the exact compressed/decompressed full receipt. No frames or cases
are dropped from either representation.

## Physical input and constructed owners

`building_anim_translucency_setup.py` enumerates physical GAPOWR body, buildup
and ActiveAnim resources from the declared expansion/RA2/RA2MD archives. Every
found GA/GT/GG filename and explicit miss is supplied to the existing named-file
transport. The original readers choose the retained image; no desired image
pointer or scalar interpretation is supplied. The selected Building rules
layers are RULESMD, optional loose LANGRULE, MPBattleMD and XMP03T4. This reuses
the shared component setup, not the complete XMP03T4 map loader.

Original BuildingType construction/readers and Country, House and Building
constructors execute. GAPOWR must retain its physical `Enslaves=NULL` result,
so its ordinary constructor does not create a SlaveManager; no slave behavior
is suppressed. The Country retains its constructor defaults. Human/current
player flags, LightGold choice, alive/placed/Guard Building pose and a sparse
flat revealed Cell neighborhood are declared priors. Full Country/House INI
loading, all stock type registrations, Unlimbo and GrandOpening are outside
this boundary.

Original selected palette expansion and full Convert construction run after
the corrected RGB565 mask startup at `4BAA73..4BAB06`. Receipts name the actual
Anim drawer/light and selected converter against the constructed LightGold
schemes and global ANIM/PALETTE converters. Palette identity is not inferred
from shade count. Original discovery-geometry initializers and the Tactical
matrix tail execute with supplied camera `(-384,380)` and a 160×160 clip.

The inherited original Scenario reset and relight use the default Ground=50
and Level=8 profile. A drawn flat level-zero Cell has top intensity950; the
Building's flash/effect callback passes that input to the slot. This is cell
lighting, not the Building ART `ExtraLight` body/bib adjustment. The GPU
component supplies the same default-profile flat-cell lighting through the
normal `CellLightGrid` builder while retaining its gameplay actors' actual map
coordinates and height. It does not claim identical whole-map lighting.

Only wholly pristine missing Object-destructor listener headers receive their
original CRT initializer prefixes, before slot creation. Established headers
and live registries remain unchanged. The prefixes stop before process-exit
destructor registration. They do not replace FindIndex or any retirement
callback.

## Notice, release and animation samples

Original `451890` creates physical `GAPOWR_A` in ActiveAnim slot 3, including
its retained coordinate, selected drawer, flags and actual Anim constructor.
Original AI advances through frames 0–7; the fixture never writes a frame,
timer, rate, forced-translucency flag or pause state to obtain a sample.
For every frame it performs:

1. A baseline draw.
2. The notice range `71B0EA..71B14E`, from a declared already-linked lone
   Temporal head, including the populated Building callback `4521C0`.
3. Four actual paused AI visits and a draw.
4. Whole `LetGo71ABC0`, including `452210` and native parent/head cleanup,
   followed by a draw and actual resumed AI.

A separate original `451890` replacement during erasure exercises the
new-slot path and retirement of the old slot. It must retain pause while
remaining distinct from the already-existing slot's forced-translucency
state. The replacement is drawn before and after LetGo, then advances through
ordinary AI. A case therefore requires 26 whole draws. Three fresh VMs use
black/A127, `0xBDF7`/A127 and `0xBDF7`/A63 underlays, all with initial Z65535.

The stock Anim type's DetailLevel2 remains unchanged. The supplied component
performance prior is constructed through original minimum/buffer setters,
60 Logic counter-prefix visits and the Throttle tail, with only `timeGetTime`
inputs 0 and 960 milliseconds supplied. This must produce FPS60. Each draw
observes the real `55AF60` threshold call/return and latch. Full Logic,
Options/Rules startup and measured machine performance are not claimed.
First-timer `atexit7C978A` registration is a recorded platform success only
from original caller `55E36F`; process-exit callbacks are not executed. Native
cdecl stack cleanup, clock conversion and Throttle timing remain intact.

## Complete draw output and comparison boundary

Whole `DrawIfVisible422C70` reaches `DrawIt422CA0`, the actual Surface call,
Convert selector, SHP decoder, clipping, row walker and selected pixel code.
The physical GAPOWR_A reader must report nonFlat and `Shadow=false`, so each
complete draw has one body call. This does not suppress a native shadow or
depend on the separate ordinary-shadow renderer change.

Each draw resets its uniform 160×160 color/Z/A inputs. `rows[].state_before`
and `state_after` record live lifecycle state; `calls[]` records the actual
frame, crop, position, clip, flags, depth, gradient, intensity, selected
converter and reached pixel routines. `rows[].calls[0].after_planes` contains
all three complete planes. In the compact JSON, each `*_words_rle` value is
a space-separated string of decimal `count:word` pairs in row-major order;
for example, `2:0 1:65535` encodes three words. Counts must be positive, words
must fit uint16, and accumulation must never exceed 25,600 words. Every
complete decoded plane must contain exactly 25,600 words. Encoding validates
these bounds and round-trips to the original run pairs.

The compact projection retains all lifecycle states, AI histories,
transactions, call arguments, palette/performance receipts and store coverage.
It removes the duplicate whole-row before/after planes only after checking
that they equal the sole Surface call's corresponding planes. The raw archive
retains both copies as ordinary `[count, word]` arrays. Full Z/A equality and
all 78 complete color/Z/A outputs remain available; no
Python or Rust blend formula supplies an expected pixel. Original text and
the listed class/selected-pixel vtables must remain unchanged.

Inherited allocation/delete/TLS, immutable file, Windows interlocked/GUID,
setup-clock and exit-registration transports are declared in the sidecar.
Mark `5F5850` and Display registration `4A9720` are recorded success sinks;
they do not provide coordinates, frame selection, palettes, draw flags or
pixels. The draw surfaces contain neither terrain nor the Building body.

This comparison establishes the bounded populated-slot lifecycle and body
draw behavior. It does not execute CLEG firing/hit admission, Temporal
construction/full AI, Display traversal, a complete map, native save/load or
a full battle. A gameplay producer test and the actual-emitter GPU consumer
are separate compositions; their validation should be reported independently.
The related [free-animation comparison](anim_translucency.md) owns SMKPUFF
selection and free-body translucency. The slot material transport uses the
shared DrawIt selector/admission result and applies only when its translucency
bits are nonzero, the Anim follows the ordinary shape path, the atlas source
is indexed, and the final palette descriptor has native rows. The inherited
`DrawState` must equal its default except for the remap row, which is preserved
in the selected packet. Inherited cloak, EMP, scissor, sinking or displacement
state, special RING1/tiled/bouncing/meteor paths, and metadata-free or RGBA
sources retain their previous material handling. These compatibility branches
are not claims of original composed-material pixel parity.

With retail archives available through `RA2_DIR` and the extracted retail INIs
in the ignored `ini/` directory, run the two compositions separately:

```powershell
$env:VERA20K_REQUIRE_RETAIL_INI = '1'
$env:VERA20K_REQUIRE_RETAIL_ASSETS = '1'
cargo test --locked -p vera20k --lib retail_cleg_attack_freezes_and_releases_power_plant_slot -- --ignored --nocapture --test-threads=1
cargo test --locked -p vera20k --lib retail_building_slot_matches_original_temporal_translucency -- --ignored --nocapture --test-threads=1
```

The gameplay fixture loads physical XMP03T4 through the ordinary scenario
loader, then places GAPOWR and CLEG through the production spawn/Unlimbo owners.
It selects level foundation and in-range infantry cells with the same
height-aware playfield predicate used by Unlimbo; geometry-only edge admission
would let Foot Approach move an apparently in-range CLEG inward first. No map
height or weapon rule is changed. `ShortGame=false` is explicit because the
added human attacker house owns only an infantryman; the normal defeat gate
must count that admitted force. The original launch player and starting MCV
remain in the scene.

Before issuing Attack, the fixture lets the initial Building operational update
recreate its `ActiveAnimPoweredSpecial` slot through the existing `4549B0`
policy, then waits for real Anim AI to advance. It retains that settled identity
through the ordinary CLEG Attack, temporal link, twelve held frames, Stop and
the due Attack callback's release. Stop is not assumed to release immediately.
At release, the same slot must have lost its temporal pause and forced
translucency, and the reciprocal temporal links must be clear.

The subsequent first Building operational restore runs the same `4549B0`
policy, separately covered by `building_power_dispatch.json`, and recreates
this powered-special slot. The resume regression therefore
requires that single expected replacement, checks retirement of the released
slot, and follows the replacement's real AI progression. It must not follow an
arbitrary replacement during the held/release interval or require the old slot
to survive the later operational transition. This is a Rust composition of the
existing operational owner with the bounded native notice/release evidence,
not original execution of a complete CLEG fight.

The GPU consumer uses the actual physical atlas, slot emitter, lowerer and
replay on BGRA/RGBA sRGB targets. Its baseline, held and immediately released
states retain the same settled slot and select native rows from the genuinely
observed animation frame. Three states, three underlays and two formats require
18 full color/Z/A comparisons. Rendering must preserve simulation state and
RNG. These are test contracts; execution results are reported separately.

A separate manual check is to attack a stock Allied power plant with a hostile
Chrono Legionnaire, then stop the attack: its active animation should freeze,
become translucent and resume after release. These commands do not perform that manual check
or a whole-scene comparison with the original game.
