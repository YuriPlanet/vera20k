# Ordinary CLEG copied chrono beam

`rad_beam.py --chrono` extends the [existing RadBeam producer](rad_beam.md).
Whole original `SpawnRadBeam6FD620(mode1)` executes with native-read CLEG rules,
fixed ART, an actual native Teleport locomotor and native ChronoBeamColor.
Its four cardinal births feed the same `RadPixels` geometry/raster owner.
No FLH, projection, attenuation, RGB565 or lifetime formula is implemented here.

From the repository root, after all authors stop and the source/input closure
is frozen, generate to scratch and independently compare in a second process:

```sh
source "$HOME/Documents/vera20k-dev/env.sh"
export PYTHONPATH=".:tools/spatial_oracle"
export PYTHONDONTWRITEBYTECODE=1
export VERA20K_GAMEMD_EXE="$HOME/Documents/vera20k-dev/game/Command and Conquer Red Alert II/gamemd.exe"
export VERA20K_PROJECTILE_RENDER_ASSETS="$HOME/Documents/vera20k-dev/class-parity/shrapnel-inputs"
export VERA20K_CHRONO_BEAM_MAP="tools/procedural_drawing_oracle/rad_beam/ordinary-cleg-chrono.map"
export VERA20K_RAD_BEAM_MAP="tools/procedural_drawing_oracle/rad_beam/ordinary-deso-radbeam.map"
python -B -m tools.procedural_drawing_oracle.rad_beam --chrono --write --output "$HOME/Documents/vera20k-dev/techno-active-retail/C30-native-v1/chrono_beam.json"
python -B -m tools.procedural_drawing_oracle.rad_beam --chrono --check --output "$HOME/Documents/vera20k-dev/techno-active-retail/C30-native-v1/chrono_beam.json"
python -B -m tools.procedural_drawing_oracle.rad_beam --check
```

[`finish_vectors`](../native_oracle.py) creates `chrono_beam.json` and its
`chrono_beam.meta.json` sidecar. With no `--output`, their paths are beside this
document. It records the actual executable identity, canonical replay command
including `--chrono`, and canonical payload hash. It checks declared imported
sources and the retained map for drift during execution; sidecars retain no
repository source hashes. Generation does not copy C29 expected outputs or
read Rust output. Only accepted, consumed native files belong in the tree.
Omitting `--chrono` still selects C29's `rad_beam.json` and its existing
metadata; the final command above checks that independently.

## Physical and typed reader boundary

The retained [ordinary CLEG map](rad_beam/ordinary-cleg-chrono.map) is 73,860
bytes, SHA-256
`cd2a316907719f79cd43f67bcad45076e107677b8327e66fcdc6e99ffeccfda5`.
An optional `VERA20K_CHRONO_BEAM_MAP` must match its entire byte sequence. Its
roster is CLEG at `(44,48)`, hostile MTNK at `(48,48)` and the existing GAPOWR;
the map is the exact final reader layer, rather than a source of component
actor coordinates. Neither a C29 map nor the C27 IFV map is substituted.

The extraction directory supplies these physical files:

| Input | SHA-256 |
| --- | --- |
| `RULESMD.INI` | `3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef` |
| `MPBattleMD.ini` | `50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963` |
| `ARTMD.INI` | `e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31` |

Missing `LANGRULE.INI` is recorded. Selected RULESMD, optional LANGRULE,
MPBattle and map sections are installed in separate passes. Fixed ARTMD is
independent of those layers. The existing lexical extractor selects exact
physical strings and preserves section existence. It does not type those
values: the original cached-INI readers do that. This is neither original
file/archive loading nor complete Rules discovery/`Process`/scenario startup.

Whole Rules constructor `665650`, InfantryType constructor `5236A0`,
InfantryType reader `5240A0`, weapon reader `772080` and warhead reader
`75D3A0` execute. Selected fields are CLEG Primary/ElitePrimary,
Image/Strength/Armor/TurretOffset/Locomotor/Teleporter; NeutronRifle and
NeutronRifleE Damage/ROF/Range/MinimumRange/Projectile/Warhead and four
beam-precedence flags; ChronoBeam Radiation/Temporal/Verses; ART
Image/Sequence/PrimaryFireFLH/Crawls/FireUp; and AudioVisual ChronoBeamColor.
The full selected `ClegSequence` is available to the original type reader.
Native referenced-type construction, lookup and registry insertion run with
declared empty registries and spare capacity. Projectile reference constructors
may execute; projectile reading, firing and detonation do not.

Both selected physical weapons must retain IsRadBeam without laser, electric
or eruption precedence, and their ChronoBeam must remain Temporal. The actual
native type GUID at `+34C` is retained and checked against CLEG's physical
Teleport GUID, and native Teleporter `+CD4` must be true (`ReadBool713FF1`,
store `713FF6`). The original GUID read `71241D` and subsequent native stores
populate the measured 16-byte field. ART supplies FLH `100,-25,135`. The physical stock color string
is `128,200,255`; Python never installs that triple as a field or beam output.

## Selected AudioVisual execution

ChronoBeamColor is exact-case `[AudioVisual] ChronoBeamColor`, at Rules
`+1866`. Original Rules constructor stores `BL=0` into its three bytes at
`66785E/667864/66786A`: black is the constructor default, independently of
retail tuning. The original reader's order places it after LaserTargetColor
and LineTrailColorOverride and before MagnaBeamColor. There is no selected
color postpass in the inspected AudioVisual body/dispatcher.

The whole AudioVisual reader is outside this component. Each layer instead
executes these explicitly declared original boundaries:

1. Enter `6691E0` with `ECX=Rules` and `[entry ESP+4]=cached layer INI`.
   Run original prologue and section lookup `526810`, stopping **before**
   `66920D` if present or `66B8F1` if absent. The native stack delta must be
   `-CC`; its section result is recorded, and all Rules bytes must be unchanged.
2. For an admitted section, enter `66B7A7` and stop **before** `66B7D2`.
   Supply only live frame inputs: component `ESP`, `ESI=Rules`, `EDI=cached
   layer INI`, and `EDX=ESP+10`. The last input is the pointer originally
   computed by `66B79D`, before the previous color's final store. The preceding
   colors and unrelated AudioVisual fields are outside this selected fragment.
3. Whole `ReadColorRGB474B50`, native `ReadString528A10` and native CRT execute.
   The fragment itself pushes the actual current Rules RGB as the default,
   supplies literal section/key pointers and copies all returned RGB bytes
   back into Rules. Output-local guards and every other Rules byte must remain
   unchanged. The whole AudioVisual function is never represented as returning.

`474B50` formats the current RGB default, uses its native 64-byte string read
and integer scan, and stores the native low-byte results without a color clamp
or palette conversion. The retained controls exercise fresh constructor,
absent section, present section with omitted key, authored `11,22,33`, then
omitted key retaining that current value. `sections`, `current_default`,
native admissions, exact read/default trace and resulting `rgb` are explicit.
They do not expand into malformed input or broad read-history coverage.

The selected color fragment runs before the selected type reads in each
component pass. That convenience order is not a claim about full native Rules
startup or `Process` ordering. Constructor/current RGB, typed physical flags
and all wrapper results come from original execution.

## Actors, births and raster controls

Source and target are explicitly supplied native Infantry-vtable `7EB058`
components, both using the native-read CLEG type, rookie rank, no passengers
and zero links. The source has initialized Facing, zero burst and a native
Teleport object constructed by whole `718000`, linked through its original
ILocomotion pointer. The target has no locomotor. This two-CLEG nonbuilding
component target is deliberate: the retained map's actual MTNK target is a
separate production Attack/app control, not silently represented by this actor.
Neither Infantry construction/Unlimbo nor a COM locomotor factory runs here.
Choosing constructor `718000` is a declared/static GUID-to-class identity
boundary based on the physical native-read GUID, not a claim that the manual
constructor call executes native COM selection.

Teleport constructor installs whole-object vtable `7F50CC`, ILocomotion
`7F5000`, and its native `+24` matrix slot is `55A730`. Whole `6FD620(mode1)`
reaches this slot through original Infantry `523250` / Techno `6F3AD0` GetFLH;
the passive nested-call observer must see actual `55A730` with this source's
ILocomotion pointer. Its native returned matrix is retained. No Walk substitute
or host matrix is used. This establishes the settled component transform,
rather than teleport movement or timing admission.

The four headings are `0000/4000/8000/C000`. Declared source location is
`[9856,12416,0]`; target location is `[10496,12416,0]`. Original getters,
Tactical matrix initializer `6D1DC5..6D1E1E`, scalar bits
`3FC25E5374344960`, source projection and setters run unchanged. The wrapper
allocates kind1 and copies its endpoints; only native mode2 writes actor links.
Mode1 keeps both links zero and does not need an attachment/death listener to
retire. Actual allocation, constructor state, GetFLH index0, coordinate getters,
source depth and setter calls are retained in `birth_rows`.

Four `geometry_rows` use the copied first native birth at progress7: clear A,
mixed A/Z, black A and partly clipped changing Z. The unchanged common
`RadPixels` owner executes whole straight draw `659650`, segment `659AC0`,
native trigonometry/geometry/projection, preclip `7BC2B0` and packed raster
`4BFD30`. Surface and pattern inputs match [C29](rad_beam.md): 160×120,
row-major little-endian u16 RGB565, background `39E7`; raw-u16 A clear127,
black0 or `[0,1,63,127,255][(x//9+y//7)%5]`; raw-u16 Z constant65535 or
`[0,32760,65535][(x//11+y//7)%3]`. Each row stores its actual camera/crop/clip.
No packed expected color or plane is authored by Python or Rust.

Each raster row retains ordered native world segments/RGB, projected packed
primitive endpoints/depth/RGB565, native clip before/after and sparse changed
pixels. The final RGB565 SHA covers the entire destination, and initial/final
A/Z SHAs cover both immutable complete planes. The partly clipped row must
actually alter accepted XY while keeping unequal original endpoint Z.
`birth_rows/input/beam` and
`geometry_rows/supplied_birth/before_progress/after` preserve C29 consumer shape.

Shared FPS/length/CoordDivide traps, duration15, registry reversal/compaction
and retirement remain consumed from `rad_beam.json`; this variant does not
duplicate those families or invent another lifecycle owner.

## Hooks, guards and limits

The same `PhysicalBirth`/`BulletReader`/`Reader`, native CRT/TLS and allocator
owners run. Existing GUID Windows transport converts the original reader's
physical string, while setup visual calls `5F8110/5F8CE0` and atexit are counted.
Color measurements permit existing CRT TLS transport, while visual/asset/GUID,
RNG and detach seams are forbidden. Wrapper measurements also forbid TLS and
all setup seams. Measured instructions and native vtables are never patched.

`RadPixels` reuses `LaserPixels`/`Rally` memory surfaces and unchanged
`ElectricPixels` allocation/return/RNG owners. Its unrelated LaserDraw
constructor runs during setup; that effect's managers, bolt/spark and RNG hooks
remain forbidden during measurement. Memory lock/stride/unlock supplies
DirectDraw storage; the actual packed raster slot executes.

Guards cover native text, source/Teleport/surface vtables, all supplied actor
and source locomotor bytes, native ID cursor, surface padding and immutable
A/Z. Existing RNG owner initializes complete Main/Scenario/Mapgen objects with
seeds31/32/33 and rejects any request or mutation. Whole wrapper birth and these
draw controls add no RNG, CdTimer, identity or detach. Binary frame200 and
FPCW `0E7F` remain explicit component inputs.

Static caller evidence separately connects successful Infantry FireAt through
Bullet Fire success and `6FF5B7` mode1 to this wrapper; the component does not
execute that caller. It does not establish ordinary Attack admission, Temporal
`temporal_detonation`/warp/rearm, a full native scenario load, network timing
events, save/load, live fog/terrain, audio, whole Tactical/GPU rendering or
other beam modes. Production consumers, app/GPU evidence and review retain
their own bounded gates; this corpus cannot close the exhaustive class goal.
