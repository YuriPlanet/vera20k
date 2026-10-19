# Ore queues and natural spread comparison

`ore_queue.py` executes the pinned retail `gamemd.exe` through the existing
[`harvest_field` fixture](harvest_field.py) and, for natural spread, the
[`TIBTRE` fixture](tibtre.py) with original Overlay construction. It supplies prior map/queue state;
the enqueue, rebuild, Cell predicates, iterator, heap operations and Scenario RNG
run original instructions. `ore_queue.json` contains 128 body cases: the retained
52 direct enqueue calls, three full `CellClass::Reduce_Tiberium` callers, five full
`TiberiumClass::GrowthProcessor` callers, 47 full natural `SpreadProcessor` calls,
16 complete `SpreadDriver_AllTypes` histories, and five growth-before-spread
driver histories. Sixteen selected constructor/INI reader rows independently
establish signed timer values and stock percentages. The sidecar records executable identity,
Unicorn version, payload hash and fixture boundaries.

```sh
RA2_DIR='/path/to/retail/Yuri' python -m tools.spatial_oracle.ore_queue --check
```

The default is read-only. `--write` deliberately publishes fresh original-execution
results; it is not a way to rebaseline Rust output. The Rust comparison lives in
[`queue_oracle_tests.rs`](../../src/sim/ore_growth/queue_oracle_tests.rs) under the existing queue owner.

## Executed behavior

The declared successful native `[Map] Size` is 4 by 4. Original
`SurfaceDataCount @ 0x0042B1F0` returns 64; original
`ToSurfaceIndex @ 0x0042B1C0` establishes the saved coordinate/bitmap mapping.
The retained backing table and Cell vtables come from the shared fixture. The
original iterator starts at `(1,4)` and stops at its first null after the allocated
diamond. This comparison does not infer iteration order from Rust.

| Entry | Executed threshold and order |
| --- | --- |
| `AddToGrowthQueue @ 0x007235A0` | Density below 11 admits the request. Array counter 53 and 54 append normally; 55 invokes `RebuildGrowthQueue @ 0x007233A0` before appending. This is signed `counter > capacity - 10`, independent of the heap reference count. |
| `AddToSpreadQueue @ 0x00722AF0` | `CanSpreadTiberium` and the receiver's bitmap admit the request first. Array counter 43 appends normally; 44 and 45 invoke `RebuildSpreadQueue @ 0x007228B0`. This is signed `counter >= capacity - 20`. Rejected requests leave old queues untouched. |
| Both rebuilds | Reset the receiver's array counter, heap and whole bitmap, then iterate cells and seed eligible cells of the receiver's own type at priority bits zero. Draw no RNG and retain every class timer. |
| Both enqueue callers after a rebuild | Append the already-admitted request and draw once. They do not repeat admission or bitmap deduplication, even when the rebuild already seeded that coordinate. |

The supplied nonzero percentage is 0.125, exactly representable at the native
INI reader float-to-double boundary. The Rust fixture uses that production reader;
this comparison isolates queue behavior from decimal parsing.

Controls cover empty and late-reference heaps, stale bitmap clearing, same-class
duplicates, cross-class receivers, a receiver with no matching overlays, densities
0/1/10/11, occupied and sloped cells, growth/spread Scenario gates, zero percentages,
and seeds 1/31/42. Growth admission is a density-only gate even when rebuilding
finds no eligible cells because growth is disabled; spread admission uses the
request cell's own type and percentage, while the rebuild uses the receiver class.
The occupied `(3,4)` cell is retained by growth reseeding and rejected by spread
reseeding. Four classes' queue and timer state is captured before and after every
case, including classes unaffected by the call.

The full-removal caller controls establish a material same-turn difference:
removing an empty Gem cell enqueues all eight Ore neighbors into the Gem receiver
and draws eight times. Removing an empty Ore cell first rebuilds and seeds all
eight Ore neighbors, appends the first admitted north neighbor again, then skips
the other seven neighbors through `Reduce_Tiberium`'s outer bitmap gate. That call
draws once. A density-11 partial-removal control calls growth admission before
reducing density; its initial density guard prevents a rebuild and any draw.

The processor controls execute `GrowthProcessor @ 0x00722F00`,
`GrowTiberium @ 0x00483710` and existing-cell `PlaceTiberium @ 0x00487190`.
The budget draw precedes growth; Place synchronously enqueues spread and can
rebuild its receiver before the processor draws its growth-reinsert priority.
The later duplicate spread admission draws nothing. A growth append counter of
55 becomes 57 through direct processor reinsertion while the spread counter
rebuilds from 44. Processor reinsertion does not call AddToGrowth's array-counter
guard. An `INT_MIN` budget draw still removes the first heap root before returning
without growth or spread.

Signed raw RNG controls 0, `INT_MAX`, `INT_MIN` and -1 execute the original
absolute/remainder instructions. They replace one word in the seeded RNG data
state, never a function return. Every case records draw count, actual draw words,
full retained RNG-state SHA-256, indices and four subsequent original draws.
Frame controls include negative frames, `2^24+1`, `2^24+4`, `INT_MAX` and `INT_MIN`.
The original `FILD i32; FSTP f32` uses the fixture's native round-toward-zero state
(`0x0E7F`). For example, frame 16777220 and seed 1 produce integer priority
16777251 but store bits `0x4B800011` (16777250), whereas a nearest f32 conversion
would store `0x4B800012`. This is an executed conversion result.

## Natural spread and driver execution

The complete `SpreadProcessor @ 0x00722440` executes over supplied valid heap
references and the original diamond. Its budget is the original truncated
`heap_count * SpreadPercentage`, clamped to 5..25, followed by the signed
absolute/remainder draw plus one. Empty heaps and a zero percentage draw nothing.
The processor rebuilds only when **heap reference count > capacity - 20**: count
44 retains the old heap, while 45 rebuilds it. An array counter of 55 with a
one-reference heap does not trigger this guard. Rebuilding uses the existing
class receiver and original `CanSpreadTiberium` predicate.

The original processor pops its first root before checking a nonpositive budget;
the `INT_MIN` control therefore retains its array/bitmap while draining the
first heap reference and draws once. For positive budgets it counts the eight
neighbors with original `CanPlaceTiberium @ 0x004838E0` before checking whether
the source can spread. A root with no target clears its spread bitmap and does
not spend the success/visit budget. A root with targets invokes original
`SpreadTiberium @ 0x00483780` with `force=0`, spends one budget unit even if source
admission fails, and directly appends the source at priority bits zero whenever
the pre-count found more than one target. That inline append has no enqueue
counter guard or priority draw. With exactly one target it neither appends nor
clears the existing bitmap. All these outcomes are compared through native
queue arrays, actual heap reference indices and bitmap cells.

Natural Spread executes its source overlay/type, density, percentage, slope,
occupancy and Scenario spread-bit gates. On success, original
`RandomRanged(0,7)` selects the start of the N..NW scan, then the first admitted
target enters full `PlaceTiberium @ 0x00487190` with amount 3. The original flat
variant draw, `OverlayClass::Constructor @ 0x005FC380`, Mark `0x005FC570` and
RecalcAttributes `0x0047D2B0` execute. Original Mark calls
`SpreadCellGerminate(0) @ 0x004818E0` at `0x005FD0EC`, rewriting the installed
cell's density from its eight neighbors before growth admission. Growth queue
admission runs before Place's final amount write and can synchronously rebuild
its existing owner. The case with
a growth counter of 55 captures the new cell's density-1 rebuild seed followed
by the already-admitted duplicate append and density 3. The ordinary-Terrain
control retains the native constructor refusal: Place still returns, appends
growth and writes density 3 while the overlay remains -1.

Three interior-hole controls select the empty `(5,4)` cell from queued source
`(4,4)`, with all eight of the hole's neighbors allocated. Original Mark's
germination reads the constructor's initial density 1 and counts native
matching-class neighbors. Eight Ore neighbors write density 11; original
AddToGrowth's compare at `0x007235C1` observes 11 and rejects before any rebuild
or priority draw. The growth queue remains empty and the full processor draws
three times. Replacing one neighbor with Gem or empty gives seven matches and
density 10; growth appends once and the processor draws four times. All three
finish with the same new overlay 103 and density 3, while their queue and RNG
continuations differ. `germinate_cell` events retain original entry/return,
matching count, MaxDensity, before/after density and return value. Natural
`enqueue_growth` events retain the actual Cell density and overlay read at the
original compare, using its resolved EAX pointer rather than a host lookup.

Target controls execute with native `GameActive=1` over the shared real
Unit/Building/Terrain vtables: live visible Buildings and spawning Terrain refuse
germination; Units, ordinary Terrain, dead Buildings and either native
invisibility flag admit it. Other controls cover existing overlay 7, non-Buildable
land, non-AllowTiberium tile, slope, bridge flags, allocated-diamond edges,
cross-class queue sources, stale empty overlays/density/slope/occupancy, three
seeds, signed frame limits and chopped priorities beyond `2^24`. Full output
records every allocated diamond cell, including empty and non-tiberium cells.
Map lookup's resident dummy is observed from its original retained coordinates;
no boundary or admission return is supplied by an observer.

Every natural `before`, per-step `state`, final `state` and explicit
`after_drains.state` now includes `dummy`, read directly from the original
resident Cell at `0x00ABDC50`: packed coordinate `+0x24`, overlay `+0x44`, density
`+0x11E`, land `+0xEC`, signed level `+0x11B`, slope `+0x11C` and raw flags
`+0x140`. Taking this snapshot invokes no map lookup and therefore cannot stamp
the dummy. These fields retain the existing prepared fixture's initial bytes;
this is not a complete MapClass/CellClass startup comparison.

Two additional diamond-edge controls queue valid Ore at `(1,4)` with either an
occupied source or the Scenario spread bit disabled. Original processor target
counting (`0x00722547`) calls neighbor lookup `0x00481810`, then original packed
map lookup `0x005657A0`, before source refusal. Both finish with dummy coordinate
`(0,3)`, consume one budget RNG word, perform no constructor and retain serial
zero. The other observed dummy fields stay zero in this fixture. The source is
reinserted once at zero priority, giving array count 2 and heap index 1. These
controls expose the lookup effect without a successful Mark's later neighbor
queries masking it. They use the same pinned executable, Size4x4 diamond,
GameActive1, seed1/frame100 and supplied first raw word zero as the surrounding
natural rows; the original lookup, admission and RNG instructions execute.

`SpreadDriver_AllTypes` starts at **`0x007221B0`**, the original function called
by Logic at `0x0055B4DC`. Its only Scenario admission gate is the Basic growth
byte at `+0x34A6`. The SpecialFlags spread bit is consumed by source predicates,
so with growth enabled and spreading disabled the due processor still draws its
budget, visits/reinserts the existing source and reloads its timer. Growth disabled
prevents all class dispatch and timer changes. The driver visits the class vector
in ascending order and always reloads each due timer after its processor, even
when the heap or percentage causes an immediate return. Reload stores the raw
signed `Spread=` duration; no fast-growth multiplier applies.

The timer histories cover running-before-due, due/after-due, paused positive and
paused zero durations, zero and negative reloads, and signed frame wrap. Four
class outputs make retained timers and dispatch order visible. The composed
histories invoke full `GrowthDriver_AllTypes @ 0x00722C40` followed by SpreadDriver
in their observed Logic caller order; growth can enqueue spread synchronously
before the later same-frame driver. The stock fast-growth control executes the
original multiplier/`Math::ftol` and reloads 2200 at 659 frames and 10000 at 2999;
the same spread reloads remain 2200/10000. Negative Growth inputs also execute
the original normal and fast-growth reload paths.

`RandomRanged @ 0x0065C7E0` contains an inline Random draw and may reject several
words for a single chosen variant. Its entry/result and every actual inline draw
at `0x0065C837..0x0065C87E` are observed. For example, seed 42's three successful
spread visits consume 14 total draws, including rejected variant words. Draw
count includes these inline words as well as calls to `Random::Next`; hashes,
cursors and four following draws compare the entire continuation. Entry events
also retain actual instruction addresses and CanPlace return values.

## Executed Overlay identity and retirement

Every natural row records the actual Scenario serial at `+0x214`, Overlay registry
count at `0x00A8EC60` and pending-finalization count at `0x00B0F6A8` before, after
and per step. Original Overlay construction calls `Create_ID @ 0x00410230` with
the object+4 argument, then original `NextUniqueID @ 0x0068BCB0` pre-increments
the cursor. Events retain the actual returned cursor and constructor's stored
object `+0x10` identity; they also observe original Mark, Object UnInit
`0x005F65F0` and AnnounceExpired `0x007258D0`. This allocation happens before the
constructor's Terrain admission, so ordinary Terrain refusal still consumes
one serial and leaves an alive, registered limbo Overlay. Constructor-return
flags and native registry counts distinguish that survivor from successful
placement's dead, queued transient. Existing-cell augmentation and admission
failures consume no Overlay serial. Two controls supply initial cursor
`0xFFFFFFFF`: both successful placement and Terrain-refused construction assign
stored ID zero through the executed wrapping increment.

Two explicit `drain_deferred=true` driver controls invoke complete original
`DrainDeferredFinalizationQueue @ 0x00725C70` after advancing the frame by one,
the ordering observed at MainTick `0x0055DE7E..0x0055DE9F`. Their `steps` remain
pre-drain output; `after_drains` holds post-drain state/cells/events, and top-level
`state` is after the final explicit drain. Successful placement's registry/pending
counts change from 1/1 to 0/0 while its consumed cursor and placed cells remain.
The original deleting Overlay destructor `0x005FDF70` and Object destructor
`0x005F3B80` run, including the second AnnounceExpired call. The Terrain-refused
Overlay remains registered in limbo with pending count zero. These controls reuse
the existing `OriginalBridgeConstructor.prepare_deferred_services` owner for
Windows SEH and mapped-memory-validated IsBadReadPtr responses. Allocation/free
are storage services; the constructor, ID allocator, logical cleanup, queue
compaction, RTTI/class decisions and destructors execute original instructions.

The explicit post-driver drain supplies a MainTick composition seam, excluding
its surrounding callbacks and admission. External pointer-expiry recipients are
empty in this fixture. Broader persistence of refused limbo Overlays, populated
observer effects and full native save/load are outside this lifecycle comparison.
The companion retail audit establishes absent CellAnim on the selected stock
TIB/GEM inputs; authored nonempty CellAnim requires its separate Anim constructor
and identity/lifecycle dependency.

## Native timer and percentage inputs

The selected original constructor member block `0x007216CF..0x007217A6`
establishes `Spread=0`, `Growth=0` and percentage bits `0x3FB999999999999A` (0.1).
The original `ReadINI @ 0x00721A50` slice `0x00721A78..0x00721AFA` executes the
Spread, SpreadPercentage, Growth and GrowthPercentage reads and stores with the
current constructor fields as defaults. Shared cached INI indexes supply lexical
strings; scalar conversion and final stores execute original `ReadInt`/`ReadDouble`.
Missing, empty and malformed durations retain zero. Negative, signed-limit,
decimal-overflow, hexadecimal and trailing-text controls retain native ReadInt
results without a clamp; `-3` remains -3 and `$FFFFFFFF` becomes -1.

The four physical retail RULESMD sections are saved with file identity and exact
lexical keys. Original reading gives Riparius/Vinifera/Aboreus Growth/Spread 2200,
Cruentus 10000, and retail `.06` percentage bits `0x3FAEB851E0000000` after the
native scanner's f32-to-double promotion. Cruentus's explicit percentages remain
zero. The `spread_driver_original_retail_reader_values` and stock fast-growth
composition use these independently produced native values. Whole layered INI
file loading and type registry discovery remain outside the cached-reader fixture.

## Inputs and limits

The successful Resize diamond, adequate queue storage, prior array counters and
heap references are supplied state. The fixture represents history with popped
entries still present in the array; it does not simulate a multi-hour game to
reach these thresholds. Its four synthetic Tiberium instances use the shared
Riparius/Cruentus overlay registry, MaxDensity 12 and declared per-case
percentages. Full Scenario/type constructors, INI/MIX loading, allocation failure
and surrounding whole Scenario startup/scheduling are outside this comparison;
the selected Tiberium constructor defaults/readers, ore Overlay constructor and
complete ore drivers described above do execute.

The shared fixture answers OS Interlocked imports and presentation sinks.
`Cell` rectangle queries at `0x0047FDE0`, `0x0047FB90` and `0x0047FF80` return empty
rectangles; radar/tactical dirtiness are service sinks. The full-removal
RecalcAttributes sink publishes declared bare land. No admission, growth, density,
queue, rebuild, iterator, timer or RNG return is substituted. The five retained
growth-processor controls use existing-cell Place. The 68 natural-spread/driver
rows inherit TIBTRE's allocator/free storage services and empty dirty sinks;
original Overlay construction/Mark and cell attribute recalculation run. Timers'
opaque `+4` padding is declared zero, including the uninitialized driver caller
local written there, and has no independent semantic claim.

Native execution establishes these bounded queue/caller and numeric cases. The
Rust corpus comparison and separate production integration checks establish their
Rust implementation. This fixture does not certify complete ore economy, whole
map load, visual output, arbitrary map dimensions or arbitrary prior histories.

## Rust owner and validation

`OreGrowthState` remains the sole queue/counter/bitmap owner. Both enqueue sites
reuse its existing per-type rebuilds; processor reinsertion retains its separate
native semantics. Spread enqueue takes one explicit receiver, removing the old
implicit-type wrapper. Rebuilding joins spawning terrain, the live ground
occupancy grid and all terrain objects through the existing object view. The
Terrain caller borrows the actual growth/spread flags, so forced placement still
creates new ore while growth-off rebuilds seed nothing. Queue resets retain
backing allocations. Serialized queue/timer ownership and snapshot layout remain
unchanged.

The historical enqueue section of the validation receipt (`tools/spatial_oracle/ore_queue.validation.json`, in git at 1b1a8b43e) records the 60-case Rust/native
comparison, the two pre-fix failing regressions, connected production callers,
9,416 passing mandatory-retail library tests (226 ignored), successful clippy,
436 Python checks (4 skipped), and the unchanged simulation field ratchet. These
are bounded executable parity and Rust integration results. The single fresh
read-only critic found no blocker; after removing its identified redundant
full-removal bitmap clear, all 33 queue and 12 Tiberium checks passed. Whole Scenario
startup and actual multi-hour histories remain outside the comparison. Other
OQ-38 residuals and the deferred VoxelAnim IsTiberium expiry/ring are separate.

Extending the shared harvest fixture's presentation rectangle sink required
refreshing TIBTRE source provenance. Original TIBTRE execution was repeated and
its payload stayed byte-identical (SHA-256 recorded in the receipt).


### Natural spread follow-up

The native active caller is `LogicClass::Update @ 0x0055AFB0`, reached by
`Main_Tick @ 0x0055DC9E`: it calls GrowthDriver at `0x0055B4D7`, then
SpreadDriver at `0x0055B4DC`, before live-object AI. Original bytes were checked
against the pinned executable. `GameActive` is established by session preparation
(`0x0052D9D7`); this is an active YR path.

The existing `OreGrowthState` driver now uses only the Basic growth gate, leaving
SpecialFlags spread admission with its existing source predicate. The existing
Tiberium registry retains raw signed Growth/Spread values through both timer
consumers. No second queue, timer, reader or admission owner was introduced.
Original numeric comparisons include negative reloads, signed wrap, stock
659/2999 fast-growth timers and all following RNG state.

New Overlay Mark attributes reach the resolved-cell and navigation owners in
the ore rung and before each emitting Terrain AI slot returns to Logic. Both
call one World publication action using the existing runtime Recalc and private
navigation owner, retaining render dirty receipts. Recalculation
can be batched at this boundary: an already-stamped cell fails target admission
at its overlay-identity gate before land is read, while growth and source gates
use density/slope. This changes no ore-loop admission or draw ordering. The
production tests check native LandType results and current path/movement costs
before later object turns. The Terrain slot regression uses the production
reader-established Clear versus Tiberium Foot/Track/Wheel costs (100/100/100
versus 90/70/50), which equal-cost synthetic rows previously hid.

Both the target pre-count and directional spread scan resolve through the
existing map identity owner before admission. Their signed-word neighbor
coordinates preserve fixed-grid aliases and shared dummy stamps, including
occupied or Scenario-disabled edge sources which never reach Overlay Mark.
The native corpus and Rust fixture observe this retained dummy without another
lookup; the fixture's allocated mask matches the supplied native diamond.

The shared supplied-state fixture builds both direct-owner comparisons and the
21 app-frame histories. The direct comparison publishes actual owner-produced
Mark dirtiness through the same cell Recalc before inspecting cells; native
expected fields never initialize this publication. Native constructor refusal
under ordinary Terrain remains covered. Original TIBTRE execution still has the
same payload bytes after the CanGerminate observer was corrected to read actual
Cell coordinates instead of inferring them from pointer arithmetic.

The receipt's `natural_spread_followup` stage records the new validation.
Whole native Scenario startup, physical save/load execution, all map dimensions,
resource image-3/4 placement and arbitrary long histories remain outside this
bounded comparison. Existing snapshot timer reset/rebuild behavior is retained;
original Load721E80 resets timers via 46B640 at721FA9..721FC2, while Rust's saved
state/restore integration is covered separately by the library suite.

The shared nonrandom germination port and signed cell-value helper now live in
`src/map/tiberium_cell.rs`, below simulation consumers. The one
`OverlayClass::Mark` port (`src/map/overlay_mark.rs`), which map load, crates,
meteor ore, bridge rebuild and live Place share, and generated final cell
attributes call that one owner.
The authored copy's separate neighbor/density tables were removed (refactor
issue #714); `MaxDensity` remains the existing constructor constant 12, without
adding a new INI reader. Rust native fixtures explicitly set the resource
OverlayType's `Land=Tiberium`, matching the original fixture's `+0x298=5`;
`Tiberium=yes` alone does not establish that independent Mark gate.

Final Rust validation after the sole critic fixes (source `1db9b166`, main
`35fb0944` integrated): 9,429 retail library tests passed, 226 ignored; Clippy
exited successfully; 613 focused ore checks and 459 Python tests passed (four
Python checks skipped). Original 128 body/16 reader reproduction passed; original
TIBTRE and bridge-constructor controls remain unchanged. Both prerequisite
regressions failed before the fixes: dummy `(0,0)` instead of `(0,3)`, and a
Terrain AI path cost of 100 instead of the current cell's 90. The fresh critic
identified those two P2 gaps; both now pass through the existing owners. No
second critic pass was run. The release AnyTown run observed 16 previously empty edge cells become
ore at density 3 in the first frame, plus four density changes among 18 TIBTRE
neighbor observations over 2,600 ticks. The 800x600 GPU readback and v5 sealed
receipts passed the production observation validator; they do not certify native
pixels or the whole Scenario. Reproduce the edge run with
`tools/map_observation.ore-spread.example.json` through the existing observation
wrapper. The `natural_spread_followup` stage in `ore_queue.validation.json` (in git at 1b1a8b43e) keeps
source/native/binary/input/result hashes and the earlier enqueue evidence intact.
The value-owner consolidation also resolves issue #713. The Mark port tests Land 5
(Tiberium) at `0x005FD0D9` for every caller (issue #993).

## Physical authored natural history

`ore_queue --authored-natural --write` (then `--check`) continues the existing
physical XMP03T4 authored setup in the same Navigation VM. Set the existing
`VERA20K_SHRAPNEL_INPUTS` and `VERA20K_ANYTOWN_INPUTS` extraction-bundle variables.
The separate consumed `ore_authored_natural.json` records original Resize field
stores, native-normalized LocalSize, explicit UI/session priors and the actual
skirmish session flags producer and later Scenario overwrite. Native seed0 is
independent of the normal production scenario's whole-startup RNG history.

Original Growth722C40 then Spread7221B0 execute on every frame0..660; original
725C70 drains after frame increment. Rows retain queue/heap/bitmap/timer state,
all three full RNG states, actual draw events and changed physical Cells, runtime
ID cursor and original Overlay/Object Mark/lifetime. Native-equal no-op snapshots
reference the preceding full state using `same_after_as_frame`; no future state
is inferred. Intervening no-op ranges are checked free of Cell writes, gameplay
events and RNG changes. The component omits other Logic/AI systems and full
native skirmish startup. Supplied missing startup registries preserve the already
populated Abstract vector. Live ore refuses any reached animation-constructor
boundary rather than inheriting the authored bootstrap's animation substitute.

`bridge_shadow_render --ore-natural --write` (then `--check`) uses the existing
raster owner with the physical XMP map layer, checks its relevant reader/source
facts against that CPU corpus, and retains two original source72,25 rasters for
density4 and5. Point/clip/A127/surface/Z inputs are explicit raster priors;
production fog/A transport and GPU drawing are independent consumer checks.
The old `--ore` and default ore queue packets remain unchanged.
