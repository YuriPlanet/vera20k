# Original FLAMEGUY component controls

`flaming_guy.py` executes the original running, destination and cell-admission
methods, with a corpus consumed by the production tests in
`src/sim/anim_class/flaming_guy_tests.rs`. A native fault fails generation;
it is never an expected gameplay answer.

Supply an external `VERA20K_FLAMING_GUY_ASSETS` directory containing unchanged
`ARTMD.INI`, `RULESMD.INI`, `FLAMEGUY.SHP` and `H2O_EXP3.SHP`. The directory may
contain other files; this producer reads only those four. Select the executable
through `VERA20K_GAMEMD_EXE` or the shared native-oracle configuration.

```powershell
python -m tools.spatial_oracle.flaming_guy --write
python -m tools.spatial_oracle.flaming_guy --check
```

Import and `--help` read no assets or executable and perform no emulation.

The controls use full original AnimType constructors and ART readers,
full Anim constructor421EA0, cell-admission4260F0, destination425D10, and the
full primary Anim AI423AC0 including FlamingGuyAI425670 and ObjectAI5F3E70.
The input section supplies physical `IsFlamingGuy=true`, `RunningFrames=6`,
`LoopCount=1` and `Rate=500`; original readers decide the stored values.
Original SplashList reader66C184..66C287 resolves the physical ordered list.

The scene is explicitly sparse: Size48x48, LocalSize0,0,48,48, constructed Cells
16..48 on both axes, flat level0, no overlay or occupants except the named
admission controls. Original Map queries, height methods, SetLocation, Mark and
Display remain enabled. The original Logic singleton header constructor
`0040CB80..0040CBB6` runs before spare storage is supplied, allowing original
Unlimbo to complete Logic admission. Already-allocated cache/vector storage is supplied;
their world allocation/startup and complete map loading are excluded.
The original thirteen-entry Anim startup group at table `0x0081252C`
initializes this translation unit's level (104), deck (416) and zero
sentinels. The existing discovery and slope-table startup owners initialize
their numeric tables independently.

Each case starts with a fresh physical FLAMEGUY constructor at the centre of
cell32,32. Named cases cover dry RNG starts, preferred Water with the native
asymmetric distance bias, bridged Water exclusion, blocked destinations,
admission land/occupation/height boundaries, and complete primary-AI running,
expiry and falling histories. The fall cases request a constructor Z of500;
the constructor must establish its actual retained position before a fall is
claimed. No duration, destination, facing, frame, velocity or expiry result is
supplied. Only declared Cell input state differs between cases.

Output records the physical reader's stored values, constructor state, every
primary-AI position/destination/stage/timer, ordered RNG requests and raw words,
all three RNG states, child-constructor arguments, and original code/vtable
integrity guards. A normal AI case must actually retire within500 visits.
Logic membership is measured from its actual `0087F77C` items and `0087F788`
count and checked against Object byte `+98`. The distinct AnimClass registry
at `00A8E9A8` retains deferred physical objects after their removal from Logic
and is not a substitute membership observation. Retained
Object FallRate is measured separately from the animation's frame rate.
Selected control rows can be consumed through production owners; no hand-derived
expected result belongs in this producer.

The sound catalog is empty. Original native sound-name resolution occurs under
that declared boundary; this is not stock audio initialization or audio RNG
parity. A Water fall's first splash has its actual SHP and reader; other listed
Splash types have physical ART but no provided image, and are not selected by
the first-entry producer. Children are constructed but are not AI-scheduled.
No retail fatal-damage producer, whole master frame, draw/raster, pixels, complete
savegame or complete stock-skirmish parity is claimed by this component corpus.

Inherited Reader transports supply exact named SHP bytes, bounds-checked heap
allocation, no-op deletion and CRT TLS. This producer installs no gameplay
return replacements and leaves native code and vtables unchanged. A required
unmodeled OS/service dependency must be recorded separately before any transport
is added.
