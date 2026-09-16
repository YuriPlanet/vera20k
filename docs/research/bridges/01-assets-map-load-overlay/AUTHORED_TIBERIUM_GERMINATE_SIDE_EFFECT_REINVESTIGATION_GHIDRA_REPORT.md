# Authored Land-5 Germination Side Effect — Ghidra Re-investigation

Date: 2026-08-31
Status: **COMPLETE for the bounded active-YR authored-load germination corridor**
System: GSI-04.12 / GSI-04.13 shared authored OverlayPack transaction; GSI-04.15 negative dependency only
Mode: `/re-investigate`, exhaustive-slice; read-only Ghidra

## Verdict

`[ACTIVE-YR: YES]` An accepted ordinary `OverlayPack` row does more than store
identity and zero state. `OverlayClass::Mark @ 0x005FC570` tests the placed
`OverlayTypeClass+0x298 Land` code. When it is `5`, Mark writes
`CellClass+0x11E = 1` and synchronously calls
`CellClass::SpreadCellGerminate @ 0x004818E0` with literal argument `0`. The call
finishes before the next packed coordinate is read. Mark ignores its return; a
`Crate=yes` row overwrites the resulting byte with `0xFF` afterward.

`[ACTIVE-YR: YES]` With argument `0`, `SpreadCellGerminate` does **not** draw RNG,
re-randomize the overlay, recalculate attributes, dirty tactical/radar state, or
touch a Tiberium queue, bitmap, heap, or timer. It resolves the receiver's
derived `TiberiumClass` index, makes exactly eight ordered neighbor lookups,
counts only neighbors resolving to that same class, and writes only the
receiver's `+0x11E` from the stock reachable table:

| same-class neighbors | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| stored state | 0 | 1 | 3 | 4 | 6 | 7 | 8 | 10 | 11 |

The native expression is
`g_OreDensityByNeighborCount[count % TiberiumClass.MaxDensity]`; retail
`MaxDensity` is `12`, while `count <= 8`, so modulo is inert for every reachable
call in this corridor. The return is `(state + 1) * TiberiumClass.Value`, but the
authored Mark caller discards it.

`[ACTIVE-YR: YES]` Each neighbor goes through native fixed-stride
`MapClass::Get_CellClass @ 0x005657A0`, not a rectangular optional lookup. A true
miss stamps the requested packed signed-i16 coordinate into the one persistent
shared dummy and returns that dummy. The helper then reads the dummy's retained
overlay identity. Therefore one same-class dummy identity can be counted once
for each missing direction; real hits do not stamp the dummy. The helper never
changes dummy identity or dummy state. Its last coordinate side effect is the
last true miss in direction order `N, NE, E, SE, S, SW, W, NW`, or the prior
dummy coordinate if all eight lookups hit real cells.

`[ACTIVE-YR: CONDITIONAL]` A positive `OverlayDataPack` later overwrites every
allocated/in-radar real state byte, so packed data normally supersedes the
germinated byte before the first whole-map sweep and the Tiberium queue rebuild.
When that data body is absent or non-positive length, no later authored-load pass
recomputes density: the per-row byte survives unless ordinary identity validation
or later Terrain placement clears the overlay. Growth/spread queue rebuilds only
test the already-stored byte and enqueue qualifying cells. They are consumers,
not substitutes for germination.

`[ACTIVE-YR MISMATCH: YES]` Current Rust parses the required Land and Tiberium
metadata and can map an overlay to a `TiberiumTypeId`, but neither current
map-owned authored placement nor the later duplicate `OverlayGrid` pack decoder
runs this helper. Both leave accepted non-crate ordinary rows at state `0` until
packed data exists. `SharedCellDummy` also lacks the overlay identity/state needed
for exact edge reads. The bridge transaction must implement germination inside
the one map-owned synchronous Mark/finalization owner and move its result through
the consumed-once finalized payload; the runtime ore spread path and the later
queue rebuild cannot stand in for it.

## Authority, scope, and exclusions

- `[BINARY AUTHORITY]` Active Ghidra program `gamemd.exe`, x86 PE image base
  `0x00400000`; executable
  `C:\Users\enok\Documents\Command and Conquer Red Alert II\gamemd.exe`, SHA-256
  `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`.
  This pass used decompile, disassembly, xrefs, and raw-memory reads only. No
  Ghidra metadata was changed.
- `[RETAIL-DATA]` `ini/rulesmd.ini` and installed loose retail maps were read
  directly. `Dustbowl.mmx` currently hashes
  `46B07F8968BE4C267CBDEC5B99CF36E9BDE98F4AC0D23B7D634ABF86E9165A79`;
  the repository's retail golden records `316` packed `TIB01` rows.
- `[LEAD ONLY]` `C:\Users\enok\Documents\OpenTS\code\overlay.cpp` and
  `code\cell.cpp` were used to navigate the inherited design. Every material
  YR conclusion below was independently checked in `gamemd.exe` and retail YR
  data.
- `[IN SCOPE]` The Land-5 Mark call site, complete direct caller set, argument,
  return ownership, exact helper read/write set, lookup/dummy semantics, density
  mapping, RNG and queue effects, authored-load timing, later overwrite/rebuild
  behavior, retail reachability, and current Rust ownership/delta.
- `[OUT OF SCOPE]` The full runtime `PlaceTiberium`, growth/spread processor
  algorithms, low-bridge procedural stamp internals, high-bridge setter
  internals, generated-map construction, and Rust implementation. They are
  mentioned only where they prove a boundary.
- `[YR EXCLUSION]` OpenTS veins, TS-only overlay families, and inherited comments
  are not parity authority. The nonzero helper argument branch is present code
  but has no caller in the complete active direct-xref set and is excluded from
  the authored argument-0 contract.

## Address and field map

| Address / offset | Verified role | Active here |
|---|---|---|
| `ReadMapOverlayPacks @ 0x005FD2E0` | One y-major/x-minor authored identity/Mark pass, then independent data pass | Yes |
| `OverlayClass::Mark @ 0x005FC570` | Ordinary identity/state owner; Land-5 germination call at `0x005FD0EC` | Yes |
| `CellClass::SpreadCellGerminate @ 0x004818E0` | Eight-neighbor same-class density derivation | Yes |
| `CellClass::OverlayToTiberiumIndex @ 0x005FDD20` | `Tiberium=yes` gate and ordered class-range mapping | Yes |
| `MapClass::Get_CellClass @ 0x005657A0` | Signed-i16 fixed-512 lookup; miss stamps/returns shared dummy | Yes |
| `InitializeDirectionOffsets @ 0x0049F2F0` | Pre-WinMain runtime initialization of N..NW offsets | Yes |
| `CellClass::RecalcAttributes @ 0x0047D2B0` | Mark common-tail and later whole-map validation/projection | Yes, but not called by the helper |
| `MapClass::InitCellAttributes @ 0x00568BB0` | Optional all-cell germination/value aggregation plus later Recalc | Fresh authored load passes `0`, so no helper replay |
| `TiberiumClass::InitGrowthQueues_All @ 0x00722D00` | Allocates and rebuilds growth queues | Later consumer |
| `TiberiumClass::RebuildGrowthQueue @ 0x007233A0` | Filters current cells through `CanGrowTiberium` | Later consumer |
| `CellClass::CanGrowTiberium @ 0x00483620` | Reads state and admits stock `0..10` when other gates pass | Later consumer |
| `TiberiumClass::InitSpreadQueues_All @ 0x00722240` | Allocates and rebuilds spread queues | Later consumer |
| `TiberiumClass::RebuildSpreadQueue @ 0x007228B0` | Filters current cells through `CanSpreadTiberium` | Later consumer |
| `CellClass::CanSpreadTiberium @ 0x00483690` | Reads state; requires `state > type_index/2` plus other gates | Later consumer |
| `CellClass+0x24` | packed signed-i16 `(x,y)` | receiver read; dummy miss write |
| `CellClass+0x44` | signed overlay identity, `-1` none | receiver and neighbor/dummy read |
| `CellClass+0x11C` | slope byte | not read by germination; queue predicates read it later |
| `CellClass+0x11E` | overlay state/density byte | receiver write |
| `OverlayTypeClass+0x298` | `Land=` enum; `5` is Tiberium land | Mark call gate |
| `OverlayTypeClass+0x2A9` | `Tiberium=yes` | class-mapping gate |
| `OverlayTypeClass+0x2AA` | `Crate=yes` | state `0xFF` overwrite after helper |
| `TiberiumClass+0x98` | class index | equality result and queue-family identity |
| `TiberiumClass+0xB8` | value per density unit | return calculation only |
| `TiberiumClass+0xE4` | maximum density (`12` retail) | signed `IDIV` divisor |
| `0x0081CD28` | density lookup bytes at dword stride | state mapping |
| `0x0089F688` | runtime-populated eight signed `(dx,dy)` pairs | ordered neighbor coordinates |
| `0x00ABDC50` | persistent shared dummy `CellClass` | miss target/read source |

## Complete active caller and argument proof

Fresh xrefs to `0x004818E0` return exactly two direct call sites:

1. `OverlayClass::Mark +0xB7C @ 0x005FD0EC`.
   `EDI` is zeroed at `0x005FC5EC`; the ordinary Land-5 block pushes `EDI` at
   `0x005FD0E2`, writes state `1` at `0x005FD0E5`, and calls the helper. No
   instruction consumes `EAX` after return. The next material branch reloads the
   type and, for `Crate=yes`, stores `0xFF` at `0x005FD105`.
2. `MapClass::InitCellAttributes +0x228 @ 0x00568DD8`.
   Its boolean parameter selects helper versus value-only accounting, but the
   helper call itself also pushes literal `0` at `0x00568DD4`. The returned
   value is added to the local total at `0x00568DE6..0x00568DEE`.

The three direct callers of `InitCellAttributes` settle later-load behavior:

- `ScenarioClass::Full_Init @ 0x00687B91..0x00687B92` pushes zero. It calls
  `Get_Tiberium_Value @ 0x00485020`, not germination.
- `MapClass::InitZoneMap @ 0x005671E1..0x005671E4` also pushes zero.
- `RandomMapGenerator::Generate @ 0x0059943F..0x0059944C` pushes one, causing an
  all-real-cell helper pass in the generated-map path. This is active inherited
  behavior but not authored OverlayPack replay.

Thus both direct helper call sites pass `0`; the compiled `randomizeType != 0`
branch at `0x00481919..0x00481955` is not reachable through the complete direct
caller set. It contains the helper's only RNG call and overlay-identity write.

## Exact helper algorithm

The instruction-level contract of `0x004818E0..0x004819F8` is:

```text
SpreadCellGerminate(receiver, randomize_type = 0):
    if receiver.overlay_id == -1:
        return 0

    receiver_type = OverlayToTiberiumIndex(receiver.overlay_id)
    if receiver_type == -1:
        return 0

    value = TiberiumClass[receiver_type].Value

    matching = 0
    for direction in [N, NE, E, SE, S, SW, W, NW]:
        candidate = wrapping_i16(receiver.coord + DirectionOffsets[direction])
        neighbor = MapClass.Get_CellClass(candidate)
        if OverlayToTiberiumIndex(neighbor.overlay_id) == receiver_type:
            matching += 1

    remainder = signed_idiv_remainder(matching, TiberiumClass[receiver_type].MaxDensity)
    receiver.state = byte_at(0x0081CD28 + remainder * 4)
    return (receiver.state + 1) * value
```

Tiny but load-bearing details:

1. The current cell is not counted; the loop performs eight neighbor iterations.
2. All eight iterations run. There is no early exit after a mismatch or match.
3. Comparison is by the derived Tiberium class index, not exact overlay ID.
   `TIB01` and `TIB20` both count as Riparius; a GEM/Cruentus neighbor does not.
4. Neighbor state/density is never read. A same-class neighbor counts at any
   `+0x11E` value.
5. The receiver's pre-call state `1` is not input to the table calculation.
6. A receiver with no overlay returns before any lookup and leaves state intact.
7. A Land-5 overlay that is not `Tiberium=yes` returns before any lookup and
   leaves Mark's caller-written state `1` intact.
8. `OverlayToTiberiumIndex` returns `-1` for identity `-1` or a type without
   `Tiberium=yes`.
9. For a `Tiberium=yes` identity outside every configured class image range,
   `OverlayToTiberiumIndex` emits the diagnostic
   `Overlay %s not really tiberium` and returns class index `0`, not `-1`.
   This value fallback is already mirrored by Rust; retail resource identities
   fall inside their normal ranges, so the diagnostic is custom-data-only.
10. The density table is byte-read at a four-byte stride. Raw memory beginning at
    `0x0081CD28` gives logical indices `0..11` as
    `[0,1,3,4,6,7,8,10,11,7,0,1]`.
11. Native has no zero-divisor guard around `IDIV`. Retail construction fixes
    `MaxDensity=12`; no retail INI key changes it.
12. The multiplication is signed integer arithmetic. Mark discards it, so value
    cannot affect authored placement, cursor state, or queues.

## Exact real-or-dummy lookup behavior

`MapClass::Get_CellClass @ 0x005657A0` loads candidate `x` and `y` as signed
16-bit values and computes `linear = y * 512 + x`. It does **not** clamp either
axis independently. It returns a real cell pointer only when `linear` is in
`0..0x3FFFF` and the map pointer-table slot is non-null. Consequently an
axis-looking coordinate can linearly alias a real slot; the test is the native
linear/pointer seam, not a Rust rectangle test.

For every true miss it:

1. copies the requested packed coordinate dword to dummy `+0x24` at
   `0x00ABDC74`;
2. returns dummy base `0x00ABDC50`;
3. lets the helper read dummy overlay identity at `+0x44`;
4. does not initialize or clear dummy identity/state.

`InitializeDirectionOffsets @ 0x0049F2F0` writes the runtime table before
`WinMain` (the cold PE bytes are zero):

| index | direction | `(dx,dy)` |
|---:|---|---|
| 0 | N | `(0,-1)` |
| 1 | NE | `(1,-1)` |
| 2 | E | `(1,0)` |
| 3 | SE | `(1,1)` |
| 4 | S | `(0,1)` |
| 5 | SW | `(-1,1)` |
| 6 | W | `(-1,0)` |
| 7 | NW | `(-1,-1)` |

The helper therefore can count one persistent dummy multiple times: each missed
direction maps the same retained dummy identity, even though its coordinate is
restamped. If the prior dummy identity maps to the receiver's class, every such
miss increments `matching`. The final dummy coordinate is the highest-index
missing candidate in the table above. If a later direction resolves real, it
does not undo the prior stamp. If no direction misses, the helper leaves the
dummy entirely unchanged.

The later real-cell `OverlayDataPack` pass never targets the dummy. Whole-map
Recalc iterates only real cells, but its own neighbor lookups can subsequently
restamp the dummy coordinate. It does not reconstruct the dummy or clear its
identity/state. Therefore helper-time dummy identity reads are synchronous and
observable to later packed rows, while the helper's final coordinate is not a
stable end-of-load ownership claim.

## Read/write and side-effect ledger

| State | Read | Written | Exact effect with argument `0` |
|---|---:|---:|---|
| receiver `+0x24` coordinate | Yes | No | basis for eight signed-i16 additions |
| receiver `+0x44` overlay identity | Yes | No | derives class; nonzero-argument-only identity write is unreachable here |
| receiver `+0x11E` state | No | Yes | table result, unless an early return leaves caller's `1` intact |
| neighbor/dummy `+0x44` identity | Yes | No | same-class count |
| neighbor/dummy `+0x11E` state | No | No | density does not influence count |
| shared dummy `+0x24` coordinate | No | On each miss | ordinary `Get_CellClass` miss side effect |
| shared dummy identity/state | identity only | No | retained process state can affect repeated misses |
| Scenario RNG | No | No | only compiled nonzero-argument branch calls RNG |
| tactical/radar dirty state | No | No | base Object Mark dirtied once earlier; helper adds none |
| Recalc/LAT/zone/cache | No | No | Mark common tail calls Recalc after helper; helper does not |
| growth/spread queue, bitmap, heap | No | No | rebuilt later from final real state |
| queue timers/frame counter | No | No | no queue/timer call or global-frame read |
| diagnostic output | Conditional | Conditional | malformed flagged range miss logs, then maps to class 0 |

The “no Recalc” row is deliberately helper-local. `OverlayClass::Mark` reaches
its ordinary common tail at `0x005FD1FA` and calls
`CellClass::RecalcAttributes(-1) @ 0x0047D2B0` after germination and the possible
crate overwrite. That Recalc does not derive density, does not read the density
byte for normal projection, and preserves it unless identity validation clears
the overlay/state pair.

## Authored-load timing and later supersession

The active fresh authored order is:

1. `ReadMapOverlayPacks` enters only for signed `NewINIFormat > 1` and a
   positive-length identity section.
2. It walks `y=0..511` outer, `x=0..511` inner. The complete constructor,
   `ObjectClass::Mark`, derived Mark, germination, crate overwrite, and Mark
   common-tail Recalc finish before the next coordinate.
3. Germination sees identities already present at that moment: earlier packed
   rows and earlier procedural low writes. It cannot see later packed rows.
4. Only after the complete identity traversal, a separately positive
   `OverlayDataPack` body blindly overwrites `+0x11E` for every native
   in-radar/allocated real cell. It neither reads identity nor touches the dummy.
5. `ScenarioClass::Full_Init` performs the first whole-real-map Recalc sweep at
   `0x00687A43..0x00687A6B`. It does not recompute density.
6. `[Terrain]` loads at `0x00687A74`. `TerrainClass::Unlimbo` can clear a
   same-cell resource identity/state before queue seeding.
7. Growth queues initialize at `0x00687A85`; spread queues initialize at
   `0x00687A8A`. Both scan final then-current real cells.
8. Units, Aircraft, Infantry, Structures, and Smudge load afterward.
9. `MapClass::InitCellAttributes(0)` runs at `0x00687B92`, computes value from
   existing state and repeats Recalc. It does **not** call germination and cannot
   repair an omitted authored density write.

The survival matrix is therefore:

| Condition after the Mark row | State reaching queue rebuild |
|---|---|
| positive OverlayData body, identity survives | packed data byte |
| no/non-positive OverlayData body, identity survives, no Terrain clear | germinated table byte |
| helper early return on Land-5/non-Tiberium type, no later data | caller-written `1` |
| `Crate=yes`, no later data | `0xFF` |
| Mark/common sweep/Terrain clears identity | cleared identity and state, not germinated byte |

## Why queue rebuild is not a substitute

`TiberiumClass::RebuildGrowthQueue @ 0x007233A0` and
`RebuildSpreadQueue @ 0x007228B0` never call `SpreadCellGerminate` and never write
`Cell+0x11E`. They iterate existing cells, derive type, invoke a predicate, and
insert `{coord, priority=0.0}` plus a bitmap bit for accepted cells.

The predicates consume density:

- `CanGrowTiberium @ 0x00483620` requires a flat recognized resource with
  `state < MaxDensity - 1` (stock `0..10`) plus its scenario/percentage gates.
- `CanSpreadTiberium @ 0x00483690` requires
  `state > tiberium_type_index / 2`, flat slope, no ground object, and its
  scenario/percentage gates.

For a flat source-order `2x2` Riparius block with no `OverlayDataPack`, native
row order produces:

```text
0 1
3 4
```

The first cell has zero earlier same-class neighbors; the second sees west; the
third sees north and northeast; the fourth sees north, west, and northwest.
All four are growth-eligible under the density threshold. The state-0 cell is
spread-ineligible for type 0, while states `1`, `3`, and `4` are density-eligible.
If Rust omits germination and initializes all four to zero, a later exact queue
rebuild has already lost the information needed to distinguish them. Queue
membership logic cannot infer the native source-order density after the fact.

## Retail reachability and INI authority

`OverlayTypeClass::ReadINI @ 0x005FE770` reads `Land=` into `+0x298` and
`Tiberium=` into `+0x2A9`. At the end, when `Tiberium=yes` and Land remains its
constructor default `Clear=0`, it forces `Land=5`. This is the bridge from stock
resource definitions to the Mark call.

Retail `rulesmd.ini` proves routine reachability:

- `[TIB01]` at lines `28187..28192` and `[TIB20]` at `28320..28325` set
  `Tiberium=yes` and do not override Land.
- `[GEM01]` at `29035..29046` and `[GEM12]` at `29188..29200` do the same; their
  apparent `Land=Rock` lines are commented out.
- The other stock TIB/GEM family rows likewise set `Tiberium=yes`.
- `[Tiberiums]` at `30372..30376` orders Riparius, Cruentus, Vinifera, Aboreus.
  Their retail `Image`/`Value` pairs are `1/25`, `2/50`, `3/25`, and `4/25`.
- The installed `Dustbowl.mmx` loads as a retail temperate `70x76` map. Existing
  direct pack golden `tests/overlay_compression_verify.rs:128..139` records `316`
  `TIB01` (`id 102`) rows. Those are ordinary Land-5 rows, so the call is a
  high-frequency stock authored-load mechanism, not a custom-only bridge edge.
- Independently decoded `Lostlake.mmx`, `Killer.mmx`, and `Shrapnel.mmx` all use
  `NewINIFormat=4` and contain both overlay pack bodies. Their data bodies
  demonstrate the common superseding case; a valid authored custom map may omit
  `OverlayDataPack`, activating the persistent-density case.

An explicit custom `Land=Tiberium` with `Tiberium=no` still enters Mark's Land-5
call gate, but `OverlayToTiberiumIndex` returns `-1`; state remains `1`. Conversely
a `Tiberium=yes` row with an explicit non-Clear, non-5 Land is recognized as a
resource elsewhere but does not take this Mark call. These are active custom INI
semantics, not stock data.

## OpenTS correspondence and YR exclusions

`OpenTS\code\overlay.cpp` supplied the correct navigation lead: its packed
identity loop constructs one overlay per row, its ordinary Mark path writes
state `1` for `LAND_TIBERIUM` and calls `Tiberium_Adjust`, and its later
`OverlayDataPack` loop overwrites real cell state. `OpenTS\code\cell.cpp` has the
inherited `_adj` table and same-class eight-neighbor calculation.

This correspondence is not evidence authority. Active YR independently proves:

- the exact call at `0x005FD0EC` and literal zero argument;
- the complete two-xref caller set;
- persistent shared-dummy reads/stamps rather than trusting the OpenTS comment
  that neighbors “off the map” are skipped;
- no helper-local Recalc, queue, dirty, or RNG effect;
- the later Full_Init queue and second-Recalc order;
- stock YR TIB/GEM definitions and authored map rows.

TS veins, weed land, and other inherited resource behavior are excluded. The
only OpenTS detail retained in the implementation handoff is one that the active
YR binary separately proves.

## Current Rust ownership and exact delta

### Existing correct prerequisites

- `src/rules/overlay_types.rs:307..314` parses `Tiberium=` and reproduces the
  `Tiberium && Land==Clear -> LandType::Tiberium` force.
- `OverlayTypeFlags` already retains both `tiberium` and canonical `land`.
- `src/rules/overlay_types.rs:470..487::tiberium_type_for_overlay` gates on the
  Tiberium flag, searches the native family ranges, and falls back to the first
  class for a flagged range miss. That matches the native value result, although
  Rust does not emit the native diagnostic.
- `src/rules/tiberium_type.rs` retains `value` and `max_density`; current
  `max_density` is the native retail constant `12`.
- `src/map/resolved_terrain.rs` already owns a fixed native lookup/allocated seam
  and one persistent `SharedCellDummy`, which is the right identity to extend.

### Current mismatch

- `src/map/resolved_terrain.rs:2438..2470` writes raw overlay identity and runs
  only high bridge stamping. It has no ordinary Mark state-0/Land-5 helper path.
- `src/map/resolved_terrain.rs:2472..2481` applies packed state when present, so
  final bytes happen to agree for retained real cells in the common both-pack
  case, but it cannot produce no-data germinated state or synchronous dummy
  effects.
- `src/sim/overlay_grid.rs:206..257::from_native_overlay_packs` decodes accepted
  rows again, assigns non-crates state `0` and crates `0xFF`, recalculates, then
  applies data. It neither checks Land 5 nor counts neighbors. This duplicate
  decoder cannot see procedural identities absent from raw packs and must be
  replaced by finalized-payload consumption under the parent design.
- `src/map/resolved_terrain.rs:421..429,784..834::SharedCellDummy` currently stores
  coordinate, level, slope, and a bridge-flag subset only. Exact helper reads
  require the same planned dummy extension to retain signed overlay identity and
  state; germination itself writes only coordinate through misses and reads
  identity.
- `src/sim/ore_growth.rs:1196..1240` has current-type and spread predicate logic,
  while runtime spread at `:1273..1342` consumes RNG and places density `3`.
  That is a different runtime transaction. Reusing its placement operation would
  add wrong RNG/dirty/queue behavior and lose authored source ordering.

### Required owner boundary

Implement one narrow map-owned `spread_cell_germinate_zero` operation inside the
authored row transaction after ordinary identity/state write and before crate
overwrite/common-tail Recalc. It must:

1. use current in-transaction real/dummy identities, not raw pack membership;
2. use the existing registry mapping and exact fixed lookup/dummy seam;
3. write only the selected real receiver state;
4. consume no RNG and emit no dirty/queue mutation;
5. let later packed data overwrite real state;
6. carry the surviving post-validation identity/state through the one
   `FinalizedOverlayPayload` into `OverlayGrid` without a second decode.

The helper should not be placed in the runtime ore queue module merely because
both mechanisms concern resources. Native ownership and timing are map/Mark
side, and the queue module is only a later consumer.

## Acceptance tests and contract implications

| # | Required fixture | Exact assertion | Failure prevented |
|---:|---|---|---|
| 1 | Flat allocated `2x2` same-class rows in y/x source order, no data body | final pre-payload states are `[0,1;3,4]`; RNG unchanged | all-zero no-data drift; post-pass rather than inline derivation |
| 2 | Same fixture with conflicting positive OverlayData bytes | data bytes replace every real germinated byte before the first whole-map sweep | treating germination as final authority when pack data exists |
| 3 | Adjacent `TIB01`/`TIB20` plus GEM | same Riparius IDs count despite differing exact identity; Cruentus does not | exact-ID or “any resource” comparison |
| 4 | Unit-level fixed-seam edge with prior dummy Riparius identity and multiple true misses | dummy counts once per miss; final coord is last missed N..NW candidate; later real hits do not clear stamp; no dummy identity/state write | rectangular `None` skip, fresh dummy per lookup, or single-count dedupe |
| 5 | Land-5/non-Tiberium custom type | caller writes `1`; helper returns zero before neighbor lookups and leaves `1` | conflating Land with Tiberium flag |
| 6 | Tiberium flagged range-miss custom type | maps to type 0 for density comparison; optional diagnostic remains non-sim observation | returning `None` instead of native fallback |
| 7 | Land-5 plus `Crate=yes` custom row | order is ordinary `0 -> 1 -> table result -> 0xFF`; helper return ignored | crate-before-helper or skipped helper |
| 8 | Instrumented accepted stock ore/gem rows | exactly eight lookups; no RNG cursor advance; no helper dirty/Recalc/queue/bitmap/heap writes | accidental runtime placement reuse |
| 9 | No-data fixture through common-tail and both whole-map sweeps | retained valid flat identity keeps germinated state; sweeps do not recompute it | relying on later Recalc |
| 10 | No-data Riparius `2x2` through queue seeding | all four meet density growth threshold; state 0 fails spread density gate while `1/3/4` pass it, subject to other gates | treating queue rebuild as density generation |
| 11 | Positive-data and missing-data controls through finalized payload | `ResolvedTerrainGrid` and final `OverlayGrid` contain the same single-owner identity/state result; zero second decode | split map/sim authority |
| 12 | Fresh authored Full_Init second-cell pass instrumentation | `InitCellAttributes` receives zero and never calls helper again | accidental post-object density rebuild |

Recommended stable test names:

- `gsi_04_13_authored_land5_germination_is_inline_and_source_ordered`
- `gsi_04_13_land5_germination_counts_same_tiberium_class_not_overlay_id`
- `gsi_04_13_land5_germination_reuses_shared_dummy_per_true_miss`
- `gsi_04_13_overlay_data_overwrites_germination_but_absence_preserves_it`
- `gsi_04_13_germination_consumes_no_rng_dirty_or_queue_state`
- `gsi_04_13_queue_rebuild_consumes_no_data_germinated_density`

## Coverage ledger

| Required question | Result | Evidence |
|---|---|---|
| Exact authored caller timing | **VERIFIED** | reader `0x005FD3F4..0x005FD51C`; Mark `0x005FD0D3..0x005FD0F5` |
| Exact argument | **VERIFIED: 0** | `PUSH EDI` with `EDI=0`; second caller `PUSH 0` |
| Complete direct caller set | **VERIFIED: 2 xrefs** | `0x00568DD8`, `0x005FD0EC` |
| Return ownership | **VERIFIED** | ignored by Mark; accumulated by optional InitCellAttributes arm |
| Receiver/neighbor reads and writes | **VERIFIED** | helper disassembly `0x004818E0..0x004819F8` |
| Exact neighbor count/table | **VERIFIED** | helper loop and raw `0x0081CD28` bytes |
| Same-type versus exact-ID/state | **VERIFIED** | per-neighbor `OverlayToTiberiumIndex` compare only |
| RNG effect | **VERIFIED: none for arg 0** | sole RNG call gated by nonzero arg at `0x00481919..55` |
| Dirty/Recalc/queue effects | **VERIFIED: none helper-local** | complete helper callee/body scan; Mark common-tail separation |
| Out-of-bounds/dummy behavior | **VERIFIED** | `Get_CellClass 0x005657A0`; direction initializer `0x0049F2F0` |
| Later data/sweep/Terrain behavior | **VERIFIED** | reader data store `0x005FD640`; Full_Init `0x00687A34..0x00687B92` |
| Queue rebuild substitution question | **VERIFIED: no** | rebuild/predicate bodies `0x007228B0`, `0x007233A0`, `0x00483620`, `0x00483690` |
| Retail reachability | **VERIFIED** | `ReadINI 0x005FE770`; retail TIB/GEM rows; Dustbowl golden |
| OpenTS comparison / TS exclusion | **VERIFIED** | corresponding source read; every material fact cold-checked in YR |
| Current Rust owner/delta | **VERIFIED** | direct source inspection; no helper symbol/path found |

## Open Questions Log — final drain

- `[RESOLVED] OQ-GERM-01 — Does Mark pass zero or one?` Zero. `EDI` was zeroed
  before the ordinary block and is pushed at `0x005FD0E2`.
- `[RESOLVED] OQ-GERM-02 — Does the helper count exact overlay IDs?` No. It maps
  every neighbor independently and compares derived `TiberiumClass` indices.
- `[RESOLVED] OQ-GERM-03 — Does neighbor density matter?` No. Neighbor
  `+0x11E` is never read.
- `[RESOLVED] OQ-GERM-04 — Are missing neighbors skipped?` No. Every direction
  calls `Get_CellClass`; true misses return and expose the persistent dummy.
- `[RESOLVED] OQ-GERM-05 — Can repeated misses count repeatedly?` Yes. Each
  iteration remaps the same retained dummy identity after restamping its coord.
- `[RESOLVED] OQ-GERM-06 — Does the helper enqueue or dirty?` No. Its complete
  body has no such call/write. Object Mark dirty and common-tail Recalc are
  separate surrounding effects.
- `[RESOLVED] OQ-GERM-07 — Does positive OverlayData win?` Yes, for every real
  in-radar/allocated cell, after the entire Mark pass.
- `[RESOLVED] OQ-GERM-08 — Is absent OverlayData repaired later?` No. The first
  sweep, queue rebuild, and later `InitCellAttributes(0)` do not recompute it.
- `[RESOLVED] OQ-GERM-09 — Is the helper's return used by Mark?` No. Fresh
  authored Mark discards it.
- `[RESOLVED] OQ-GERM-10 — Is this TS-only?` No. Stock YR TIB/GEM rules force
  Land 5, and retail authored maps contain packed resource rows.
- `[DEFERRED, NON-BLOCKING] OQ-GERM-11 — Should Rust reproduce the malformed
  custom-data diagnostic text?` The sim value result is settled (fallback type
  0). Diagnostic transport/presentation is not a bridge simulation behavior and
  can remain a loader-observability decision.

No unresolved binary, retail-data, ordering, side-effect, dummy, or Rust-owner
question remains for this bounded mechanism.

## Zero-add and cold spot-check pass

After forming the contract, a fresh pass repeated the load-bearing checks:

1. Re-decompiled `SpreadCellGerminate @ 0x004818E0` and independently read its
   full disassembly, including early returns, argument gate, eight iterations,
   `IDIV`, table byte, receiver store, and return multiplication.
2. Re-read all xrefs to `0x004818E0`; only `0x00568DD8` and `0x005FD0EC`
   remain.
3. Re-disassembled `OverlayClass::Mark`; confirmed `state=1`, zero push, ignored
   return, crate `0xFF`, and later common-tail Recalc order.
4. Re-read `0x0081CD28` raw bytes and the runtime direction initializer rather
   than treating cold-zero `0x0089F688` memory as the active table.
5. Re-disassembled `MapClass::Get_CellClass` and confirmed signed fixed-512
   linearization, pointer-null miss, coordinate stamp, and persistent dummy.
6. Re-disassembled Full_Init from pack reader through both queue inits and the
   later zero-argument `InitCellAttributes` call.
7. Re-decompiled both queue rebuilds and both density predicates; no density
   writer or helper call appeared.
8. Re-read retail rules and current Rust owners; no second Rust germination
   implementation or later reconstruction path appeared.

The pass added no new material mechanism and changed no verdict. No Cargo command
was run.

## Stale-document wording to replace

1. [docs/research/CELLCLASS_PLACETIBERIUM_FUN_00487190_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_PLACETIBERIUM_FUN_00487190_GHIDRA_REPORT.md)
   correctly says authored map load does not call `PlaceTiberium`, but lines
   `234..236` overextend that into “no map-load seed caller” and call
   `0x004818E0` a spread-queue post-germination step. Replace with:

   > Authored OverlayPack load does not call `CellClass::PlaceTiberium`.
   > However its ordinary `OverlayClass::Mark` path synchronously calls
   > `CellClass::SpreadCellGerminate(0)` for every accepted Land-5 row. The
   > helper derives the current real cell's initial state from eight current
   > same-TiberiumClass neighbors and has no direct queue effect.

2. [docs/research/OVERLAY_CLASS_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OVERLAY_CLASS_SYSTEM_GHIDRA_REPORT.md) lines `564..568` say the
   helper is xref'd/called from the spread-queue processor. Replace caller
   provenance with the exact two xrefs: `OverlayClass::Mark @ 0x005FD0EC` and
   `MapClass::InitCellAttributes @ 0x00568DD8`. Its density pseudocode is broadly
   correct but needs persistent-dummy and argument-0 reachability notes.
3. [docs/research/PAVEMENT_AND_TILE_PROPAGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PAVEMENT_AND_TILE_PROPAGATION_GHIDRA_REPORT.md) lines
   `173..186` and `341..343` infer that `SpreadCellGerminate` grows ore and should
   call/retrigger Recalc/LAT. Replace with: the helper only derives the receiver
   state byte and return value; its complete body has no Recalc/LAT/dirty/queue
   call. Surrounding Mark or runtime placement owners may recalculate separately.
4. [docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md) density table remains
   correct. Add exact authored caller provenance, same-class/dummy semantics,
   zero-argument no-RNG behavior, and the fact that queue rebuild consumes rather
   than generates the byte.
5. `AUTHORED_OVERLAYPACK_INLINE_TRANSACTION_REINVESTIGATION_GHIDRA_REPORT.md`
   intentionally left the Land-5 helper internals unexpanded. This report closes
   that extension without changing its y/x ordering, OverlayData, or whole-map
   Recalc verdicts.

## Ghidra annotation candidates (not applied)

- `CellClass::SpreadCellGerminate @ 0x004818E0`: plate candidate
  `arg0 active callers only; 8 N..NW real-or-dummy lookups; same derived tib
  class; receiver state table write; no queue/dirty/Recalc; Mark ignores return`.
- Mark call `0x005FD0EC`: EOL candidate
  `Land==5 authored row calls SpreadCellGerminate(0) after state=1; Crate FF is
  later`.
- `g_OreDensityByNeighborCount @ 0x0081CD28`: data candidate for the twelve
  dword-stride logical values `[0,1,3,4,6,7,8,10,11,7,0,1]`.

These are worker-report-only candidates. No rename, comment, type, or other
Ghidra metadata mutation was made.

## Sources

- Live active-retail `gamemd.exe` Ghidra decompile/disassembly/xref/memory
  inspection at the addresses listed above.
- Retail `ini/rulesmd.ini`.
- Installed retail `Dustbowl.mmx`, `Lostlake.mmx`, `Killer.mmx`, and
  `Shrapnel.mmx`; repository retail overlay golden.
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAYPACK_INLINE_TRANSACTION_REINVESTIGATION_GHIDRA_REPORT.md`.
- `docs/research/bridges/01-assets-map-load-overlay/OVERLAYPACK_SHARED_DUMMY_FINAL_RECALC_FIELDS_REINVESTIGATION_GHIDRA_REPORT.md`.
- [docs/research/bridges/01-assets-map-load-overlay/GDIRECTIONOFFSETS_0089F688_BRIDGE_MARKER_PATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/GDIRECTIONOFFSETS_0089F688_BRIDGE_MARKER_PATH_GHIDRA_REPORT.md).
- [docs/research/TIBERIUMCLASS_MAP_LOAD_QUEUE_SEEDING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBERIUMCLASS_MAP_LOAD_QUEUE_SEEDING_GHIDRA_REPORT.md).
- `C:\Users\enok\Documents\OpenTS\code\overlay.cpp` and `code\cell.cpp`
  (navigation lead only).
