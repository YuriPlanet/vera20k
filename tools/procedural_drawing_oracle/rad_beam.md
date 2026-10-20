# Ordinary DESO direct RadBeam

`rad_beam.py` executes original `gamemd.exe` instructions for the ordinary
mode0/kind1 beam. It composes the existing cached INI, native constructor,
math/RNG, memory surface, allocator and raster owners. Python supplies declared
component inputs and records native outputs; it implements no FLH transform,
beam geometry, FPS expression, trigonometry, clipping or pixel formula.

The opt-in [`--chrono` variant](chrono_beam.md) uses this same producer for CLEG
mode1 with separate inputs and output files. Omitting that flag retains this
DESO corpus, its schema and its provenance independently.

From the repository root, after freezing the producer and its imported sources:

```sh
source "$HOME/Documents/vera20k-dev/env.sh"
export PYTHONPATH=".:tools/spatial_oracle"
export PYTHONDONTWRITEBYTECODE=1
export VERA20K_GAMEMD_EXE="$HOME/Documents/vera20k-dev/game/Command and Conquer Red Alert II/gamemd.exe"
export VERA20K_PROJECTILE_RENDER_ASSETS="$HOME/Documents/vera20k-dev/class-parity/shrapnel-inputs"
export VERA20K_RAD_BEAM_MAP="tools/procedural_drawing_oracle/rad_beam/ordinary-deso-radbeam.map"
python -B -m tools.procedural_drawing_oracle.rad_beam --write
python -B -m tools.procedural_drawing_oracle.rad_beam --check
```

`--write` produces `rad_beam.json` and `rad_beam.meta.json` beside this document;
`--check` compares newly executed output without changing them. The shared
[`finish_vectors`](../native_oracle.py) owner records the canonical replay command and payload hash. It rejects source/input changes
during a run; sidecars retain no repository source hashes. Numeric goldens must
come from that execution, not this document or Rust output.

## Physical input and admission boundary

The retained [ordinary map](rad_beam/ordinary-deso-radbeam.map) is 73,860 bytes,
SHA-256 `565b61d34519b501a3d7b249fd7566e0621832719402b04a369df405e38f87b5`.
An optional `VERA20K_RAD_BEAM_MAP` must be byte-identical to it. This map is the
final rules layer; no Hills map is substituted. The extraction directory must
contain the following physical files; missing `LANGRULE.INI` is recorded.

| File | Physical SHA-256 |
| --- | --- |
| `RULESMD.INI` | `3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef` |
| `MPBattleMD.ini` | `50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963` |
| `ARTMD.INI` | `e1f0378394313c04ebbd5073f47785ee3e46f1b3c62d65724e8f3c310ee7ba31` |

[`BulletReader`/`Reader`](../projectile_oracle/bridge_render_inputs.py) owns
cached INI indexes, allocation, native CRT/TLS and asset transport. Its lexical
extractor selects physical strings; native readers parse all measured values.
Separate passes run for RULESMD, optional LANGRULE, MPBattle and the exact map.
Fixed ARTMD is installed independently of those layers. Section existence is
preserved when selected keys are absent, so a native admission return remains
observable. These are filtered reader calls, not native file/archive loading or
a complete Rules `Process`, type discovery or scenario startup.

The original Rules constructor `665650`, InfantryType constructor `5236A0`,
Radiation reader `66CF70`, InfantryType reader `5240A0`, weapon reader `772080`
and warhead reader `75D3A0` execute. Selected physical fields are DESO
Primary/ElitePrimary, Image/Strength/Armor/TurretOffset; both direct weapons'
Damage/ROF/Range/MinimumRange, Projectile/Warhead and effect flags; warhead
Radiation/Temporal/Verses; ART Image/Sequence/PrimaryFireFLH/Crawls/FireUp; and
Radiation RadColor. The selected `DesoSequence` remains available to the whole
type reader. Other sections/scalars and visual/sound startup are outside the
measurement. Empty supplied registries have declared spare capacity, while
native factories construct, find and insert referenced types. Referenced
projectile construction may execute; projectile readers, firing and damage do
not belong to this component.

The physical direct-beam gate is guarded: both weapons must read IsRadBeam,
without laser/electric/eruption precedence, and RadBeamWarhead must remain
non-Temporal. Native RadColor defaults are read from the actual constructor,
whose `6677EF/6677F5/6677FB` stores initialize black. A separate sequential
control runs absent section, missing key, authored `11,22,33`, later missing key
and wrong-case key. Each call preserves the actual current RGB as its native
default argument. No green fallback is supplied by Python.

## Retained controls

| Payload family | Original execution and declared boundary |
| --- | --- |
| `inputs` | Selected constructors/readers above, physical lexical strings, exact layer hashes, read defaults/admissions and before/after fields. |
| `color_controls` | Fresh Rules constructor plus five sequential exact-case/default/current-value controls. |
| `birth_rows` | Six whole `6FD620` mode0 calls, actual fixed-ART index0 FLH and native getters/projection: four cardinal headings, positive-depth synthetic ART and equal-raw-Y synthetic ART. |
| `fps_rows` | Fourteen whole `6591B0` manager calls: startup0, low/30/high current rates, unsigned high-bit/max, requested0 and negative requested controls. Each stores reciprocal qword bits, full ftol EDX:EAX, spacing bits and progress. |
| `length_rows` | Eight original `659650` prefixes ending before `65978C`: zero/short/nonintegral length, signed low32 square/sum overflow and coordinate extremes. Actual squared dword and resulting length bits are retained; these are explicitly not whole draws. |
| `division_rows` | Six whole `65A1B0` calls: zero, positive and negative divisors with signed coordinates/extremes; no divide-overflow exception is hidden. |
| `geometry_rows` | Eight whole straight draws at declared progress7: physical birth, mixed A/Z, black A, partly clipped changing Z, short/count0, nonintegral overshoot, zero length and disabled drawing. Native world endpoints/RGB, clip before/after, ordered packed primitive arguments, sparse pixels and whole-plane hashes are retained. |
| `lifetime_rows` | Three manager histories: duration15, reverse two-entry duration2/3 compaction and wholly clipped duration15. They retain all 36 visits, native registry order, surviving states, cumulative delete observations and unchanged RNG/identity. |

Birth actors are supplied Infantry vtable `7EB058`, the native-read type,
rookie rank, coordinates, zero burst, zero endpoint links and no passengers.
Native Walk constructor `75AA90` and Facing initialization execute. The Tactical
matrix uses the same original `6D1DC5..6D1E1E` initializer as the existing laser
fixture; projection scalar bits and viewport remain declared inputs. This does
not execute Infantry construction, Unlimbo, purchase, placement, House, AI, input
or fog admission. The ordinary source is `[9856,12416,0]`, the nearby nonbuilding
target `[10496,12416,0]`, and fixed physical ART authors FLH `100,-25,135`.
Synthetic FLH strings go through the original type reader before the wrapper.

Native wrapper `6FD620` calls allocation `659110`, constructor `6593F0` and the
original setters. Call traces retain constructor state, actual GetFLH index0,
source/target coordinates and source adjustment. The raw-Y unequal branch can
produce either adjustment sign; raw-Y equality bypasses projection. No endpoint,
FLH, depth result or measured native return is supplied. Registry insertion and
growth execute. Mode0's two actor links remain null; a source-death listener is
not required to retire these copied endpoints.

`RadPixels` reuses [`LaserPixels`](../spatial_oracle/building_prism.py) and
[`Rally`](rally.py), borrowing unchanged `ElectricPixels.allocate`,
`return_boundary` and `RngTrace` from [electric_bolt.py](electric_bolt.py).
The inherited unrelated LaserDraw constructor runs during surface setup; its
managers never run. No bolt or palette VM is constructed. Draw components use
original allocation/setters with independent native wrapper results, except
explicit supplied endpoint/progress/duration controls.

## Numeric and raster interpretation

Manager `6591B0` reads unsigned current rate `ABCD44` and signed requested rate
`A8B558`. Its reciprocal of the cubed ratio is spilled to a qword at `65926C`
before multiplication by10 and ftol. The observed value is not replaced with a
single host expression. Ordinary requested rate30 comes from PrepareSession
`52DABD`; the network timing-event writer and its evolution remain open.
FPCW `0E7F` is explicit. Native `7C5F00` converts to i64: invalid conversion
retains low32 zero, which matters for current0/requested0 controls.

Length uses wrapping low32 squares/sum before signed conversion and original
SqrtApprox `4CAC40`. The segment divisor is ftol(length/spacing), but the
original do/while loop continues while `++index*spacing < length`; nonintegral
length can emit one segment beyond that floor divisor. Short/count0 and zero
length retain masked native nonfinite math. Whole segment `659AC0` runs the
original sine table `4CACB0`, minLength360, radian constant, truncated Z offsets
and `0.5 + 0.5*abs(truncated displacement)/configuredAmplitude` RGB factor.

The surface defaults to 160×120, row-major little-endian u16, background
`0x39E7`. A is raw u16: clear127, black0, or mixed
`[0,1,63,127,255][(x//9+y//7)%5]` as owned by `Rally.alpha_at`. Z is raw u16:
constant65535 or the supplied list indexed by `(x//11+y//7)%len(z)` as owned by
`LaserPixels.__init__`. Every geometry row records its input pattern, native
projected crop origin, camera and clip rectangle. Final RGB565 SHA covers the
entire plane; initial/final A/Z SHAs cover the complete immutable planes.
Sparse pixels are witnesses, not a replacement for the whole destination hash.

Original straight draw preclips projected XY through `7BC2B0`, then calls
packed RGB565 `4BFD30` with untouched endpoint Z arguments. The partly clipped
row must actually change accepted XY and retain unequal endpoint depths. The
shared packed raster, including its second clipping pass, executes unchanged.
No Python/GPU image supplies expected pixels. BSurface memory lock/stride/unlock
stands in for DirectDraw storage; the original DSurface packed draw slot runs.

## Guards and limits

The producer rejects changes to original text, source/native surface tables,
guards, A/Z, native ID cursor and all three full RNG objects. Main/Scenario/
Mapgen seed prestates31/32/33 use the existing RNG owner. Birth/lifetime
families retain full before/after RNG bytes; every measurement rejects RNG or
detach calls. Native mode0 has scalar duration/progress, no CdTimer and no
Abstract identity. Lifetime visits deliberately keep binary frame200 fixed:
reverse manager traversal, expiry, stable compaction and deletion are measured
per tactical draw, including clipped visits. Scenario clear/save-load policy is
not established by these histories.

Reader allocation/delete, CRT TLS and GUID Windows transport are inherited
setup boundaries. Type visual entries `5F8110`/`5F8CE0` and atexit return only
during setup, with counts recorded. These, asset lookup and TLS/GUID seams are
forbidden inside measured wrapper calls. Draw allocation/deletion/atexit and
memory surfaces use existing transport owners; inherited Spark/bolt hooks are
guarded unreachable. Measured math, getters, constructor, registry, manager,
segment, clipping and raster bodies receive no replacement return values and
no instruction bytes are patched.

These controls do not execute a full FireAt/world shot, timing-event stream,
native scenario load, GPU upload, whole Tactical composite, live terrain/fog,
audio, damage, RadSite, eruption/kind2, Chrono or magnetic attachment mode.
Production Rust/GPU integration and review establish their own bounded evidence;
this corpus alone cannot close those gates or the exhaustive class audit.
