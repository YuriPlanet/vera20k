# Original human barracks output replay

`python -m tools.spatial_oracle.factory_infantry_output` is the public caller and
comparison owner for the bounded human GAPILE → two E1 loop. Its preferred
`no_rally` and `rally` controls execute the original registered Foot EMPTY and
Walk startup, then continue both produced GIs through terminal movement, mission
and reciprocal radio cleanup. This is native evidence for the stated caller and
inputs; it does not certify a whole match or whole object.

The package imports existing type, terrain, Sound, construction, Factory, radio,
Walk and runtime owners. Six retained caller modules compose those owners;
`initialized.py` adds original registered startup and observation to that same
composition. No replacement native gameplay function, executable image, retail
asset or copy of a shared helper is included.

`meta.json` pins P1–P10 seals, original and current caller/support identities,
four preserved 59-file helper profiles, original instruction/data/vtable spans,
physical inputs and lossless native projections. Unknown helper versions fail
with the changed path and observed/expected SHA. Register another profile only
after comparing the original native controls. The preferred current profile is
`c60-compatible`, with SHA256
`d968facdfffc7248fae4d5c91469f30fe8e28e08d2888a92f2e0d2384345c977`.
The existing `tools/native_inspect.py` instruction observer has its own
`inspection_owner_files` pin and participates in the import census. It is checked
separately so the accepted gameplay/helper profiles remain unchanged. Failed
initial portable runs that lacked this observer pin are retained outside the
package; their complete native comparisons matched, but their CLI status is FAIL.

## Configure physical inputs

Use the existing `tools.native_oracle` configuration: set `VERA20K_GAMEMD_EXE`
to the executable, or `RA2_DIR` to its directory.
Unlike the other oracles, this sealed packet still requires the original YR 1.001
file: its sources are pinned by hash, so its file-identity checks stay until the
packet is next refreshed. Selected instruction/data/vtable spans are checked too.

Supply an existing extracted input directory with `--assets` or
`VERA20K_FACTORY_INFANTRY_OUTPUT_ASSETS`. `meta.json:physical_inputs` pins its
exact listing: 65 present entries and absent `LANGRULE.INI`. The shared Reader
reads every root file, so extra, missing or case-colliding entries fail. The
closure includes audio.idx/audio.bag, rules, ART, mode, map, selected SHP/TMP and
inherited fixture inputs. It is a physical-read closure, not a claim that every
file affects barracks gameplay. The tool neither extracts nor copies assets and
has no machine-local evidence-directory fallback.

Selected layered-input SHA256 identities:

- `RULESMD.INI`: `3d341ef8a13a4b5ab24af2eef48ac94931ac2bb87d950fe3330a07e2d25672ef`
- `MPBattleMD.ini`: `50406e81d7523f6be1954daab6b25bd85a8347c455f3d53dcf515d7f719b4963`
- `Hills.mmx`: `96de259a741f1fc4d0d4bc4de1a1582755e90a77c2404978814149113fc42f22`

Existing input owners execute original constructors/readers through rules →
absent language rules → Battle mode → Hills map. ART and Sound are separate,
unlayered inputs. Full map, theater, menu, options and device startup are omitted.

## Check and reproduce

From the repository, with the binary configured and Python/Unicorn installed:

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output --check
PYTHONDONTWRITEBYTECODE=1 python -O -m tools.spatial_oracle.factory_infantry_output --check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output --check --assets /path/to/extract --output /existing-directory/new-check.json
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output --replay no_rally --assets /path/to/extract --output /existing-directory/new-no-rally.json.gz
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output --replay rally --assets /path/to/extract --output /existing-directory/new-rally.json.gz
```

`--check` validates saved evidence and original bytes without emulation. Its
explicit exceptions remain active under `-O`. It also verifies that the production
fixture `src/sim/world/fixtures/factory_infantry_local_native.json` equals the
single mechanical `fixture.local_fixture` selector over both initialized native
projections. It does not regenerate or validate Rust behavior.

Whole original emulation requires normal Python and Unicorn 2.1.4: imported
established owners retain `assert` guards. The recorded native runtime uses
Python 3.13.2. Selected whole-Infantry visits have an observation limit of
100 million instructions / 500 seconds; inherited checked-execution limits are
otherwise retained. A limit or fault is a failed observation, never a native
answer. Full available primary/private observations are retained on replay
failure, including the checked runner's failed endpoint.

Outputs use exclusive creation; `.gz` uses deterministic gzip with mtime 0.
Preferred replays retain the complete regenerated initialized control, primary,
private GI observer, compact projection and comparison. Keep outputs outside the
input directory and this package, preserve failed controls, and never derive
native goldens from Rust. No Cargo, Git, asset or Ghidra mutation is performed.

The initialized compact projections total 273,992 compressed bytes. They retain
original startup, producer construction/admission/input closure, queue admission,
selected milestones, complete request/advance order, all complete native RNG
buffers, private Walk/mission/latch/timers and both terminal products. Their pins
cover the **complete** normalized regenerated primary/private receipts, including
fields outside the compact selection. Only declared host/source identities in
`runtime.HOST_KEYS`, runtime metadata at `queue.runtime`, `environment.runtime`
and `native_environment.runtime`, prior comparison metadata, vtable source path
and physical-root path prefix are excluded. These identities are recorded or
checked separately. No gameplay state, timer, call PC or RNG field is excluded.

## Executed startup and measured schedule

Before selected GAPILE/E1 object construction, the caller uses original
dispatcher `7CBED3` for registered Foot EMPTY slot `813100..813104` → `4D3100`,
then the whole 14-entry Walk group `81572C..815764`:
`75A880,75A8A0,75A8D0,75A8F0,75A910,75A930,75A950,75A970,75A9B0,75A9E0,75AA10,75AA40,75AA50,75AA70`.
Original startup `7CD8B4` → `_cinit 7CBDAF` passes the full C++ range
`812000..815DA4` to this dispatcher; this control invokes only the stated slices.

Original `75A970` computes and writes Walk height `B45C28=104` at `75A99B`.
Original `75AA50` initializes Walk EMPTY `B45BE8/EC/F0`; `4D3100` initializes Foot
EMPTY `8B3DA8/AC/B0`. Executed startup makes no RNG request or advance; complete
three-stream state and inherited FPCW `0E7F` remain unchanged. Other group values,
original calls/writes and reached global consumers are recorded. No scalar
height, EMPTY coordinate or x87 answer is assigned by this caller. Full process
FPU startup is omitted; the inherited FPCW remains an explicit prior.

The existing caller establishes selected whole producer ctor, original type
registration, native Unlimbo/House admission, yard HELLO before child Unlimbo and
child C/yard BREAK afterward. It executes selected producer construction,
header/mission and attached Anim visits. Opening finishes at 50, ready 51, with
no supplied producer opening field. The admitted authored yard at 8,12 is then
removed through original expiry/deferred removal. The House power consumer leaves
GAPILE at 14,14 with power 0/drain 10. Full building-factory charge/click/HousePlace
ingress is a linked prerequisite, not established by this admission caller.

Each subsequent frame visits Strip `6A8B30`, already delivered Infantry `51BAB0`
in delivery order, held Factory `4C9B20`, then local `6474C7..6474BF` OutList/DoList
dispatch. New output receives its first live Infantry visit next frame.
Completion is 267/484, PLACE 268/485 and first occupied-building Scatter/live
visit 269/486. Final House/Factory/Strip holder cleanup is synchronous within
PLACE 485; later freed arena bytes are not live Factory ownership. No stage,
balance, exit admission, movement, timer, RNG draw or cleanup answer is supplied.

No-rally reaches automatic PerCell/radio release and NULL navigation at 306/522;
both products finish Guard with Archive/navigation NULL, radio/tether clear and
private Walk destination, paid head, moving and motion cleared. Actual producer
constructor/opening RNG precedes queue admission: Main/Scenario seed 2 and MapGen
seed 31 are setup inputs, and reseeding at ready frame 51 would change the state.
`Scenario+214` is a separate recorded field, not an inferred RNG index.

Rally executes original `443860` for authored cell 20,20, including passable search
and event 1E. Exit retains the producer Archive/rally and initial exit destination.
Correctly initialized Walk completion clears exit navigation during contact
release 297/514; the same original post-Walk `520F40` → InfantryIdle `520F92` →
shared Foot idle consumes Archive and starts rally movement. Final Walk completion
is 475/704, Guard commits 476/705. Both GIs finish in actual cell 20,20 at 705 with
the same terminal movement/radio cleanup. Complete Main/Scenario/MapGen buffers,
caller identities and request/advance order are compared, including the changed
arrival/Guard Scenario cadence versus the historical controls.

## Bounds and preserved historical controls

Material supplied/excluded inputs remain: inherited human American House,
current-player/difficulty 0/alliance/options; flat 33×33 clear interior/rock
border and map diamond 16; original physical Clear01 cache/recalc; unrelated
prior GAPOWR 10,10, MTNK 11,8 and their live Anims. Those actors already exist when
the selected added startup runs; other Foot CRT slots are not rerun because they
would reset live inherited registries. Later producer/House/unrelated AI, whole
CRT/WinMain/MainTick, complete map/theater load, networking/full ring capacity,
checksum, render and audio timelines are excluded. A canonical Rust `advance_tick`
visits more actors/phases, so its structural regression cannot claim absolute
frames or whole-world RNG parity. Its HouseTechLevel 10 and ready producer are
explicit structural priors; no-rally Scatter destination equality is excluded.

HousePlace calls selected producer `EDI` vt+48 → `447AC0` at `4FB5FB` and receives
`[3968,3840,0]` at `4FB5FE`. Reached radar `65FA70`, caller `4FB636`, receives kind
6 and cell 15,15 on both PLACE visits. This establishes original producer-based
notification coordinates. **The inherited EVA registry is empty and RadarEvent
CRT `65F9B0` is omitted.** Original radar follows its nonpositive-increment refusal;
native marker insertion/Tick/cleanup and loaded UnitReady voice queue/cleanup are
not established. They require the existing EVA/radar/clock consumer owners.

The original P2/P5 receipts and projections remain byte-identical. Their loop
omitted Walk CRT and used zero `B45C28`, so they are historical supplied-prior
controls, not stock completion timing. Optional reproduction uses
`--replay historical_no_rally` or `--replay historical_rally` with the same asset
and output options. Historical rally stops at 515 after Archive handoff 298/515
and does not prove both terminal arrivals. Preserved cold continuation/faults
remain in P9; preferred corrected terminal evidence comes from sealed P10.
`fixtures/ordinary-input-closure.json.gz` remains an immutable mechanical
selection of the historical no-rally receipt. It is not the corrected selector.

P1 supplied-stage54/balance200 busy/death/removal controls remain bounded; they
are not relabeled as completed production. Specialized products, negative
coordinates, legacy planning, slave/team/Agent/VehicleThief/default-GuardArea
branches and complete startup retain separate residuals. No whole-object
completion is claimed by this tool or its saved checks.

## Registered UnitReady consumer supplement

The same public tool owns four original controls under `--replay
unit_ready_consumer`: `cadence`, `device`, `buffer` and `radar` (default `all`).
`consumer.py` invokes original readers/constructors, House notification suffix,
Clock, queue, Stream, worker and radar bodies. `consumer_setup.py` promotes the
selected GI/Sale caller prior once. `NativeAudioPlatform` in
`engineer_repair_admission.py` remains the single RawFile/Win32/DirectSound
transport owner. No native queue, clock arithmetic, decoder, callback, payload
exhaustion or marker decision is reimplemented.

This supplement has a **separate41-entry physical input profile** in
`consumer-meta.json`. Its legacy `Hills.mmx` alias contains the recorded AnyTown
`XMP03T4.MAP` bytes; it does not change the initialized factory loop's65-entry
Hills profile. Fixed EVAMD is selected through the existing `stock.mix` owner
from `ra2md.mix -> localmd.mix`; only actual DialogList62 `EVA_UnitReady` is
admitted to the native registry. The winning Allied9354-byte WAV, SHA256
`6567dae06a071ab4985cad0b20b0c2596f923baf04a6bec097c8a9f0d937ee67`,
is supplied as an immutable prepared RawFile from the recorded production
winner `langmd.mix -> audiomd.mix`. Native raw-first CCFile/WAV/parser/decoder
bodies execute; native MIX mounting/traversal is excluded.

Use existing physical files; the tool does not extract or modify retail assets.
All output paths must be new. `.gz` losslessly retains the complete result with
deterministic gzip; a failed comparison remains a failed result.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output \
  --consumer-check
PYTHONDONTWRITEBYTECODE=1 python -O -m tools.spatial_oracle.factory_infantry_output \
  --consumer-check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output \
  --replay unit_ready_consumer --consumer-control all \
  --consumer-assets "$UNIT_READY_INPUTS" --wave "$UNIT_READY_WAVE" \
  --output /tmp/vera-unit-ready-current.native.json.gz
```

`VERA20K_UNIT_READY_CONSUMER_ASSETS` and
`VERA20K_UNIT_READY_CONSUMER_WAVE` are equivalent explicit inputs. Whole original
emulation requires normal Python because inherited fixture assertions are part
of its admitted input closure; normal/optimized saved checks use explicit
exceptions. The authoring-only `--candidate-helper-profile` flag exposes a
pinned, **unregistered** helper candidate. It cannot register compatibility.
The historical 59-file maps stay unchanged. A new compatible map requires
complete four-control native comparisons and both initialized movement
comparisons, not changed golden gameplay values.
The additive `unit-ready-compatible` 59-file map records those six completed
comparisons at source base `f1783f83d6e9d9b9df5046d7b3d83478115a278b`.
A later changed helper version is rejected until its own complete comparison;
the existing maps are retained.

The additive `c60-compatible` map records fresh comparisons at source base
`c60d196d548800cec5494ab1d48f5e022719bb81`. It preserves the three earlier maps.
The c60 integration changes none of its 59 helper files or the separate
inspection owner relative to the 9bee candidate. Relative to the f178 accepted
map, only `anim_bouncer_launch.py`, `building_construction.py` and
`building_slot_replacement.py` differ; their exact identities and bounded
reader audit are in `consumer-meta.json:candidate_profile_audit`.

The complete initialized no-rally and rally controls, four original consumer
controls and full PCM observation match their sealed counterparts. The first
combined consumer receipt remains **FAIL**: its device control timed out inside
the original decoder. Its successful cadence, buffer/PCM and radar controls
are retained, and only the failed device control was repeated with unchanged
inputs and the inherited 10-second/2-million-instruction limits. That complete
retry passes. The profile records those selected results and their raw receipt
hashes; it does not relabel the failed combined receipt or broaden coverage to
unreached helper branches. Registration changes support metadata, with every
golden and both Rust exports unchanged.

The native source packet is sealed by manifest SHA256
`ee7444412ea59b8db7b37ab4b045a68050345597c2b1f3f70c2daf7f2e81addb`.
Its historical58-source profile differs from the current owners in seven files;
those identities and source references remain explicit. Comparisons remove
only declared host driver/AST/import provenance. Every native state field,
PC/order, complete three RNG buffers, timer, queue, channel/backend/device
bytes and file payload stays in the complete normalized observation hash.
The historical 59-file profiles and controls are never rewritten.

`fixture.py::unit_ready_consumer` is the sole mechanical selector. Its full
export is `fixtures/unit-ready-consumer.json.gz`; `lean=True` serializes the
same selected data to `fixtures/unit-ready-consumer-rust.json`. Saved checks
regenerate the lean export from the retained full selection and require exact
equality. Rust tests can `include_str!` the plain export without another
selector, parser dependency or handwritten clock constants. The full export
retains all complete RNG and device-storage bytes; raw regenerated receipts
can remain outside the checkout, pinned by their complete hashes.

The controls preserve different premises: cadence uses an explicit native
StreamStop; device supplies a legal stopped status; buffer advances idealized
OS quarter cursors until original code itself asks Stop, then observes its
next worker callback; radar supplies every consecutive frame0..200. The
native-created worker's registers/stack/context survive each real Sleep seam.
The clock executes original QPF/QPC conversion using literal OS inputs,
independently of unchanged simulationframe0. The notification suffix receives
producer packed cell15,15 from the upstream preserved native observation; it
does not rerun the House local gate/Exit/GetCoords in this consumer supplement.

Hardware concurrency/cursor capture, measured WAV duration, audible output,
whole match/House/MainTick/render scheduling, other469 EVA lines and priority
conflicts remain outside these controls. Radar is a bounded type6 lifetime
proof with its inherited zero radius/pixel geometry, not stock screen geometry.
Backend+44 kind4 is a raw native format kind, **not** the WAV compression tag;
actual408754 admits physical WAVtag0x11 and backend+50 compressed flag1.
The original factory P10 cold-EVA/cold-radar bounds remain literal, even after
this separately initialized consumer supplement. No whole object or whole
match completion is claimed.

`controls.buffer.decoded_pcm` is the stable path in both exports for the whole
selected sound's literal native PCM. It contains36,612 bytes as `bytes_hex`,
SHA256 `e6c8dad9417324cff801a92a74ccf24a73e4acb5db55373669e46baae49d98a5`,
one channel,22,050 samples/second and2 bytes/sample. The same selector copies
the source-chunk boundaries, source/block/callback return identities, original
call order and coverage limits from the sealed witness. This count comes from
all18 successful original40AA70 returns at409F5A with literal observed writes,
then original409DE0 output/copy ranges. The initial45,056-byte ring contains
this complete PCM followed by silence; its capacity or a later backend field
alone was insufficient to establish valid PCM.

`pcm.py` installs optional read/write observations before the existing buffer
caller's paused1000ms pump. It supplies no native state or return and implements
no decoder. The complete original four consumer comparisons remain unchanged;
the buffer replay additionally compares every original source/block/callback
observation, including output writes and returned counts. Saved normal/-O
checks validate the retained witness and regenerate `decoded_pcm` through the
same selector. The324,510-byte deterministic gzip witness is
`accepted/unit-ready-pcm.native.json.gz`, sourced from PCM manifest SHA256
`35506b020966eb0b3ec065522473d4b02a951613149c05d190aa052c4e6b59f1`.
The production decoder stays with `assets::ima_adpcm`; consumer Rust checks
read the plain export through the existing retail fixture owner.

Raw JSON receipts retain their actual encoding hashes. Complete comparisons
use canonical JSON dictionaries while preserving array and call ordering;
inherited set-backed physical-section dictionary order may vary. This does
not permit dropping gameplay, returned PCM, file payload or RNG fields.

## Infantry Unlimbo Gate and incoming coordinates

The same public tool adds `--gate-check` and `--replay infantry_unlimbo_gate`.
`gate.py` calls the original Infantry51DFF0 frontend through the existing
`passenger_escape` fixture and observation owner. Original incoming membership,
floor lookup, Cell481180, Building47C4D0/4525F0, Door4A51B0, placement, Scenario
RNG, class occupation mark and class return execute. No native decision body
or shared fixture is copied into this package.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output \
  --gate-check
PYTHONDONTWRITEBYTECODE=1 python -O -m tools.spatial_oracle.factory_infantry_output \
  --gate-check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output \
  --replay infantry_unlimbo_gate --output /tmp/vera-infantry-gate.native.json.gz
```

This control needs the configured exact executable and existing Python native
runtime, without a physical asset-directory argument. Its native ReadBool uses
the literal Gate key and prepared cached INI values from the retained production
query receipt `inputs/infantry-unlimbo-gate-retail.json`. RULESMD authors Gate=yes;
absent LANGRULE performs no read, and absent mode/map keys inherit the prior byte.
The original constructor XOR/store slices establish default false. The query's
retail hashes and historical path are provenance; the path is never opened as a
fallback. The query is supplied reader input, not a freshly executed app loader.

`gate-meta.json` separately pins the historical14 imported native owners and
the additive `c60-gate-compatible`14 profile, SHA256
`7a27d92ec5c767f61f3078d24093a6e59a2b104c7a2756b1c6ed292bd2e191d1`.
Two complete public original replays match all18 historical controls; their
raw receipt identities are retained in the profile registration. Unknown
helper/caller versions fail. Authoring-only
`--candidate-gate-helper-profile` admits that pinned candidate for comparison
without registering compatibility. It is restricted to Gate operations. The
four accepted factory/consumer59 maps, their exports and the original receipts
stay unchanged.

The retained complete native receipt is deterministic gzip at
`accepted/infantry-unlimbo-gate.native.json.gz`, from manifest SHA256
`68c0abc23dc667a0de7f7ad7538365d00b8beba32c4d7b44674b0b982ad1f828`.
All18 complete original controls are compared after removing only the two
top-level source identities `driver_sha256` and `imports`. All native bytes,
calls, order, map/object priors, RNG requests and three full1012-byte RNG buffers
remain in the comparison. Original executable sections are checked before and
after each row. Replays preserve the complete regenerated receipt, source map,
mechanical selection, comparison and any fault; output creation is exclusive.

`fixture.py::infantry_unlimbo_gate` is the single selector for
`src/sim/world/world_spawn/fixtures/infantry_unlimbo_gate_native.json`. Saved
normal/-O checks require exact source identity, original spans, retained receipt
and selector equality. The fixture includes17 inputs representable by Rust's
position authority and one oversized negative linear alias as native-only range
evidence. It also preserves positive linear alias and -1/-128 lookup versus
placement-origin observations; the selector computes no expected gameplay.

These controls use a prepared32x32 sparse map and supplied broad map bounds,
actor/type/House state, active cell-list byte, global height/EMPTY/spot/speed
priors, and stable Gate Mission/Door state after the actual Door constructor.
Only Scenario is originally seeded31; Main and MapGen retain their supplied
mapped priors and are observed unchanged. The inherited `passenger_escape`
observer supplies the Foot4D7170 success handoff, writing location/mark/limbo;
whole Foot/Object/Techno/Idle Unlimbo is outside this proof. Full Gate constructor,
Door/mission scheduling, water behavior and whole startup remain separate
bounds. These prepared Gate controls do not relabel P10's33x33 loop or establish
that GAGATE_A (retail TechLevel=-1) is normally buildable.

## Strip publication and admitted rally order

The same public tool adds `--phase-check` and two bounded replays,
`publication_before_strip` and `publication_after_strip`. `phase.py` extends
the existing `initialized.generate` caller with one authored Building
WhatAction/ClickedAction input before or after the whole Strip visit in
frame268. It changes only the stop to the first actual PLACE and removes the
two-product terminal guards for this shorter control. Native construction,
payment, registered Foot EMPTY/14-entry Walk startup, Factory, Strip, event
append/dispatch and Infantry Unlimbo execute through their existing owners.

```sh
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output --phase-check
PYTHONDONTWRITEBYTECODE=1 python -O -m tools.spatial_oracle.factory_infantry_output --phase-check
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output \
  --replay publication_before_strip --assets "$FACTORY_INPUTS" \
  --output /tmp/vera-publication-before-strip.native.json.gz
PYTHONDONTWRITEBYTECODE=1 python -m tools.spatial_oracle.factory_infantry_output \
  --replay publication_after_strip --assets "$FACTORY_INPUTS" \
  --output /tmp/vera-publication-after-strip.native.json.gz
PYTHONDONTWRITEBYTECODE=1 python -m unittest tools.tests.test_factory_infantry_output
```

The source packet is sealed by manifest SHA256
`a0416992e1d4536b0961c742619c9aa183604126bc5294e0e78c99c3cb4e7856`.
Its two complete original receipts are copied byte-for-byte into
`accepted/publication-phase-native-{before,after}-strip-attempt-1.json.gz`.
The pinned physical/source closure and26 original instruction spans are under
`inputs/publication-phase-*`. No new shared-helper profile is introduced;
the existing `c60-compatible`59 map is preserved. `meta.json:publication_phase`
pins the original driver/selector, initialized parent seal, complete normalized
observations and the complete pre-input startup/opening/input/round prefix.
Both controls match every field in all216 original P10 rounds52..267.

Whole original Building WhatAction447540 returns action1. Its vt1404436F0
ClickedAction calls443860 at4437A8/return4437AD and appends native SetRally1E.
Strip6A8B30 calls4C9C60 at6A8DD3/return6A8DD8, consumes Factory+5D=1 and
appends PLACE0B. Local event execution4C6CB0 returns to64C916 in insertion
order. Before-Strip therefore executes `[1E,0B]`: the new GI inherits rally
Archive20,20, keeps the exit Nav and starts Walk in Move. The sibling executes
`[0B,1E]`: the already placed GI has Archive/NavNULL, Guard and stopped Walk,
while the producer receives the same rally. The class click draws no RNG;
the complete original successor-constructor draw and three buffers are retained.

`fixture.py::publication_phase_local_fixture` is the single mechanical
selector for `src/sim/world/fixtures/factory_infantry_publication_native.json`.
Saved normal/-O checks require both semantic and exact serialization equality
with its6,322-byte sealed local selection. Replays compare every original
observation using the existing, explicit host/source-path normalization;
arrays, event bytes, returns, prior state, native route and RNG stay literal.
The focused Python guards reject altered event order, missing native returns,
changed executable guards, incomplete RNG and changes outside the local selector.

The legal before-Strip window edge is instruction-established through
MainTick55D3A2/55D3B6→message pump→GameWndProc777640→TacticalMouse6930A0
WM_LBUTTONUP→BandBox4AB9B0→Selection4AE750→vt1404AE8C4. The control executes
the actual Building receiver and native event append; it does not execute
window/pixel discovery or whole MainTick. The inherited restricted loop is
Strip→delivered Infantry→Factory→localEvents, not a whole-world equivalence
claim. Both phase controls stop after firstPLACE268, before the next InfantryAI.
Networking, full event-ring capacity, other actors and later movement retain
their existing separate bounds. All older corpora/goldens/helper maps and
initialized terminal controls remain unchanged.

Cell subcell startup is also inherited explicitly: `factory.py:45` calls
the pinned `building_death_anims.joined_fixture`, which reads CRT812B28 and
executes original48E480 before construction. That owner records all60 bytes
at89E9F0 before/after and the complete three unchanged RNG streams in
`f.subcell_startup`; it supplies no subcell offset answer. The phase input
closure pins that helper at SHA256
`2c411ebe503db16d01c3a799cb3f227d901ee2c3bc488ecb442fd90cfedcd303`.
The phase primary does not separately export `f.subcell_startup`: execution
coverage here rests on the unchanged caller/source owner, rather than an
inference from the nonzero placed coordinates or a fresh60-byte phase readback.
