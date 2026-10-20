# Jumpjet Process and synchronous Unit destination evidence

`jumpjet_states.py` executes the original `gamemd.exe`.
The schema-2 payload keeps the ten former rows in `legacy_rows` and adds sixteen
`composed_controls`. The old canonical subtree SHA256 is
`43c492a0299ab35088312e4706f956eba2346f37a0c64ba7b54e9ed8f8b56d21`;
the producer checks it on every replay. The two profiles have different seams.
The legacy profile retains its historical supplied FNPC, callback and list
answers. It is not evidence that the composed callback can be deferred.

`jumpjet_states.json` is the native-derived expected projection. The existing
`finish_vectors` publisher also writes `jumpjet_states.raw.json.gz` and its
`.raw.meta.json` receipt. The receipts pin the entire decompressed corpus and
the expected projection. Gzip uses level 9, an empty filename, MTIME 0 and OS
byte 255; its receipt records the compressed hash, zlib runtime, original
binary, sources and coverage. `--check` compares both payloads and both
metadata records exactly. Rust tests consume the expected JSON without
decompressing the archive.

The superseded v3 raw evidence was 95,226,695 bytes, SHA256
`c6934aafe7130e97beb17e80fe882e1bebfddea2bd41a797b444698eee911f33`.
Its composed fixture omitted Object CRT startup and therefore left
`AC13C8=0`. At takeoff Z10, the native marked/high-flight predicate then
reported Air and skipped CellPut. That cold fixture is retained as diagnostic
evidence, not the expected initialized layer behavior. The ten legacy rows
have their separately declared historical seams and remain byte-exact.

## Reproduce

Use the native-oracle Python environment and the active-retail executable as
described in [native_oracle.md](../native_oracle.md). Use the existing release
asset owner to extract physical inputs and produce a **comparison** report:

```sh
TASK_ASSET=$(python -m tools.cargo_run --resolve asset --profile release)
"$TASK_ASSET" extract RULESMD.INI --out target/asset/jumpjet-shad
"$TASK_ASSET" extract MPBattleMD.ini --out target/asset/jumpjet-shad
"$TASK_ASSET" extract XMP03T4.MAP --out target/asset/jumpjet-shad
"$TASK_ASSET" ini-type SHAD --domain rules --map XMP03T4.MAP --mode-id 1 > target/asset/jumpjet-shad/final-type.json
PYTHONPATH=. python -m tools.spatial_oracle.jumpjet_states --check
```

The selected install has no `LANGRULE.INI`; the report and native input witness
record its absence. If present, extract that physical layer too. Override the
two input locations with `VERA20K_JUMPJET_INPUTS` and
`VERA20K_JUMPJET_STOCK_REPORT`. `--write` deliberately publishes a changed
reference; use it only with the native execution lane and source freeze.
`finish_vectors` hashes every loaded local Python owner before generation and
refuses to publish if one changes. The metadata pins binary and canonical payload
identity. Rust comparison sources require their own final binding/validation.

The stock report is never used to initialize native scalar values. Original
Rules `665650`, UnitType `7470D0`, Country `5113F0`, Jumpjet `54AC40` and Link
`54AD30` supply construction/default state. Physical text enters the existing
INI-cache owner; the selected original SHAD read regions, full Land reader
`674000`, AI BlockagePathDelay region `673A0C..673A37` and Country reader are
recorded per layer. The result is compared to the production report. This is a
selected reader witness, not full native type/ART/scenario loading.

Composed preparation reuses `object_flight_height.initialize_object_scalars`
to execute all fourteen original entries from table `8141D8` before Mark.
Original `5F37C0..5F37F0` writes Object level scalar `AC13C8`; body SHA256 is
`538627be4408437f2f94177671c17e8f9dca00b5f4859579d705ac6e79185d85`.
`5F3860` produces Object bridge scalar `AC13BC`. The existing Object query
corpus establishes 104/416. These are separate storage from Map and Jumpjet
startup. `input_world.object_startup` records the actual table and scalar
before/after values; no height scalar or layer result is supplied.

## Composed execution

The ordinary stock history starts at frame 100, seed 31, Unit coordinate
`(2688,2688,0)` and an actual Cell `(14,10)` order. The foreign Unit occupies
air slot `(12,10)`. `Unit::SetDestination 741970` calls Foot `4D94B0`, which
calls Jumpjet MoveTo `54B1C0`; the actual FNPC `56DC20/56E7C0` runs. Each
Process `54AEC0` reaches its original true RET `54B199`, with its next Update
and state dispatch in the same VM. No reached gameplay body is replaced.

At frame 151, occupied State1 synchronously calls Unit -> Foot -> Jumpjet,
then saves the **post-callback** marked byte, calls Mark(0), restores the byte
and writes phase 3 (`54BB32..54BB55`). Free State1 performs the same Mark/restore
suffix after its successful claim and writes phase 2 (`54BB69..54BB8C`).
The ordinary taken history continues through the new destination and arrival;
the free history continues into State3. The Stop history invokes the actual
Unit null-destination setter and continues to landing.

`stock_taken_state1_arrival_scenario_seed0` retains that ordinary taken history
through `process_50`, then executes `scenario_load_seed0` before the following
Process visits. The original Scenario load `689470` reads its raw block at
`6894B7` and calls post-read `683560` at `6894C5`. That post-read body pushes
zero at `683564`, selects Scenario `+218` at `683566` and calls Random Seed
`65C6D0` at `68356C`. The control imports `SEED_FN` from the existing
`rmg_oracle/gen_rng_vectors.py` owner and invokes that original body in the
live composed VM. Call/return, receiver, argument and all three complete RNG
states are recorded by the same observer. Main and Mapgen are not reseeded.
The Seed body `65C6D0..65C77A` (including RET4 at `65C777`) is 170 bytes,
SHA256 `60889d008c03c4b02b53ccaa67109bd2997602bdfcaba8e31003ad168bd0df1b`.
This is a declared RNG load handoff followed by ordinary arrival, not whole
native Scenario Save/Load. The existing `native_id_snapshot.py` owner runs
the actual save/read/post-read call chain; `building_repair.py` already uses
the same bounded original Seed(0) handoff for snapshot continuation.

Supplemental controls enter original State1/3/4 with explicitly recorded
retained inputs. They cover marked/unmarked, same NavCom, deployed refusal,
power-off and skip-MoveTo gates; State3's **live post-setter** LandType tail
with old/new water disagreement; and free/taken State4 suffixes. Unmarked
handler inputs do not establish that an ordinary Process reaches State1
unmarked. The power-off control still writes Foot destination/timers while
skipping the Jumpjet MoveTo call; its name does not imply a whole Unit refusal.

Setup reuses the existing Unit fixture, Mark/discovery, nearby-zone-plane,
air-tracker and display-vector owners. The supplied world is 32x32 flat Cell
storage, with recorded map metadata and supplemental LandType overrides;
tracker extent is 40x40. It is not an AnyTown full map load. The supplied live
foreign Unit uses real vtables/slot/tracker bodies, not a complete foreign Unit
constructor or AI history. Ordinary Process retains supplied GameActive=0;
the separate retirement control supplies GameActive=1. House current/human,
Country, TubeIndex=-1, distinct native NullCells and all other premises are
recorded. Platform seams are Windows GUID text transport, an empty FS SEH
chain, checked IsBadReadPtr and CRT atexit registration.

`input_world.registered_tile_count=0` declares the empty native IsoTile
registry (`A8ED2C` pointer and `A8ED38` count are zero-filled BSS). Cells begin
with tile index `+38=0` and overlay `+44=-1`; no physical TMP is registered.
`land_type_override_source="cached_cell_ec"` means supplemental Water inputs
change cached LandType only. Recalc `47D57B..47D595` invalidates tile0 against
count0, writing FFFF; the no-overlay fallback `47DB48` writes Clear and
`47DB52` writes slope0. Rust fixtures must bind the empty resident catalog and
actual retail TerrainRules, not a fabricated water TMP or terrain speed table.

## Shared cache, slot and landing bodies

`Foot+560` is the tracker cell; `Foot+564` is the remembered air-slot cell.
They are observed independently. Air Add `4134A0` and Remove `4135D0` call
the original tracker-cell setter `41C160` themselves (returns `4134CA` and
`413601`). Update `4138C0` changes +560 before comparing buckets (`4138DA`,
`41399A`, true RET `413A29`). Within a bucket, a Cell crossing changes +560
without removing/appending tracker membership. Across buckets it appends in
native order. The ordinary history records both kinds of crossing.

Cell `487D70` refuses a foreign claim without notification. A release notifies
the old Foot with NativeNull before clearing E0; a claim writes E0 before
calling concrete Foot `4E00B0`. The Foot notification directly clears an old
matching Cell E0 at `4E0104`, then writes the new +564. It does not recursively
call Cell `487D70`. Consequently, claiming the same slot for its current owner
returns success but clears that E0 and retains +564. An empty release does not
notify that stale owner. The slot history records normal occupied release,
replacement, foreign refusal/release and this same-owner quirk.

State2's remembered-cell lookup uses **+560**, while Cell's notification changes
**+564**. Ordinary landing/Stop execute both actual Unit occupation leaves
`7441B0/744210` (+F0/+F4). Object SetZ `5F6060` brackets its +A4 store with
Mark(0)/Mark(1) only when +74 was marked; it does not call Foot SetLocation or
add rider callbacks. Marked state and the cell-list/occupation writes remain
in the native histories.

State2's same-position test compares X/Y, then a non-null TarCom exits before
deployer/balloon work (`54BD80..54BD9B`). Its simple-deployer branch tests Unit
RTTI 1, type +E13 and owner piggyback +6AD. The DeployToLand/no-deployed/no-
piggyback arm writes target height from class +2C, saves +74, calls Mark(0) at
`54BE19`, restores +74 at `54BE22`, writes phase 2 at `54BE28` and calls
`705D60`. Otherwise it releases the current slot only if E0 belongs to this
owner, writes phase 4 at `54BED4` and calls `705D60`. The non-deployer
BalloonHover arm saves +74/Mark(0) at `54BE81`/restore at `54BE8A`/phase 2 at
`54BE90`; it does not write target height. Non-balloon uses the same owned-slot
release/phase-4 tail. State2 makes no landing-latch +90 write. These stock-false
deployer/balloon branches are instruction-order evidence, not executed retail
reachability. Original span `54BD30..54BFE4` SHA256 is
`9181a877018421e5665e7445b936f69f0a9481b80017c2df84c096edd741f773`.

State3's occupied target/balloon callback (`54C342`) continues into the live
destination tail without a Mark/restore/phase write. Successful target claim
uses Mark(0) at `54C36A`, restores +74 at `54C373` and writes phase 2 at
`54C376`. The simple-deployer occupied arm likewise has no suffix; its free
claim writes target height, then Mark/restore/phase 2 (`54C2A4..54C2CB`). The
simple-deployer arm is static evidence. State4 water/beach occupied and free
controls execute their respective Mark/restore/phase-3 and phase-2 suffixes
(`54C5F3..54C610`, `54C62C..54C64B`).

Ordinary touchdown remains phase 4 through Mark/location/occupation callbacks,
destination/moving clear, PerCell(2) and the class NULL setter. It releases the
owned current slot and removes the tracker before writing phase 0 at `54C9E4`;
crate and crash/deploy resets follow. Landing-latch +90 is set at `54C727`
before +F0 at `54C731`. MarkAllOccupation REMOVE `54D930` selects retained
destination for any nonzero phase: with a non-null head it requires either
phase 0 or a set latch, calls +F4 at `54D996`, then clears the latch at
`54D99C`. Null head skips that clear. The nonzero-phase REMOVE branch is
instruction-order evidence; the retirement control executes its phase-0 arm.

Whole active-game UnInit `4DE5D0` executes Limbo/expiry and appends the retired
object to the original deferred vector. It clears +560/tracker/Mark but retains
+564 and its occupied E0. The control then executes only original destructor
XOR `4D3595..4D3597` and cache block `4D3632..4D366E`: if its remembered slot
still belongs to this Foot, `4D3668` clears E0 directly. Obsolete +564 bytes
remain; no notification occurs. This selected block does not establish whole
destructor, deferred-drain, team, sound or locomotor-release behavior.

## Observation and limits

Every **raw archive** boundary retains whole 0x700-byte Foot, 0x98-byte Jumpjet, all three full
0x3F4-byte RNG states, scalar destination/phase/timers/facing, NavCom/Aux and
ordered queues. `memory_baselines` plus each boundary's `memory.changed_spans`
losslessly reconstruct whole Cell, world (including foreign Foot), Map, House,
air/display headers and deferred storage; reconstruction is checked. Active
cell lists/air order remain explicit. No opaque padding is converted to a
Rust gameplay field. `NativeCallTrace` matches PC **and** ESP plus callee cleanup,
so a nested call sharing a return PC cannot finish its ancestor. The original
CMIN consumer uses this same matcher with unchanged outputs.

The expected projection retains all fifteen original controls and their 375
steps unchanged, plus the additive Scenario Seed(0) arrival control. It keeps
boundary 0 and every step's before/after state; counts and both payload hashes
are recorded in the publication receipts.
`steps.before/after` index the retained array; `boundary.raw_boundary_index`
identifies the full-archive row and `output.raw_boundary_count` records its
original size. Every retained scalar field and all three complete RNG strings
are unchanged. Call/write/draw receipts retain their original order. The
projection decodes actual saved Cell +124/+128, NextObject +30 and air/display
vectors into occupation words and ordered actor/foreign identities. Unknown
pointers fail projection. It also retains the supplied foreign Foot input
fields, current TarCom identity, actual invocation arguments and supplemental
inputs decoded from the recorded supplied writes. No poststate supplies a
fixture input, and no raw padding becomes a Rust field.

Whole original .text and selected vtables remain unchanged; class-tail guards
and FPCW 0E7F are checked at every boundary. This corpus establishes bounded
native behavior. Rust tests may compare represented fields and declared fixed
precision; it does not certify whole-object, State5 crash, bridge/building-top,
full mission/AI/radio or whole-scenario parity.
