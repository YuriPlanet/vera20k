# InitCellAttributes Tag-Line, Lighting, Opaque-Zero, and Wall-Owner Tail - Ghidra Report

Target: `MapClass::InitCellAttributes @ 0x00568BB0`, newly exposed non-overlay tail only
System: GSI-04.12 / GSI-04.13 transaction-3 ancillary ordering; generic triggers, transaction 20, and transaction 21 are routed owners
Report path: `docs/research/bridges/01-assets-map-load-overlay/INITCELLATTRIBUTES_TAG_LINE_LIGHTING_TAIL_REINVESTIGATION_GHIDRA_REPORT.md`
Date: 2026-08-31
Mode: `/re-investigate` exhaustive-slice, read-only except this report
Research status: COMPLETE
Parity status: OPEN under the owners identified in the implementation handoff

## One-Line Verdict

`InitCellAttributes` first clears `Cell+0x140 & 0x300000` on every real cell, then uses AttachedTag event kinds `0x19`/`0x1A` to stamp map-bounds rows/columns for active `FootClass::PerCellProcess` trigger acceleration; these bits are not bridge state. The same per-cell tail performs a real scenario/light-source recomputation (neutral only for `(0,0)`/`(-1,-1)`), zeros a persisted but otherwise dormant/unknown `Cell+0x30` pointer slot, Recalcs, and only then reconstructs a current wall's owner. Transaction 3 owns the exact ordered seam, one light-cache invalidation, negative bridge non-ownership, and reuse of the existing post-Recalc wall owner—not generic trigger execution, LightConvert output parity, or `+0x30` semantics.

## Investigation Contract

### Target questions

1. What exactly do the `0x100000` and `0x200000` `Cell+0x140` bits mean, how are they cleared and restamped, and who actively consumes them?
2. What are the exact predicate, precedence, bounds, shared-dummy, and dual-bit behaviors?
3. What does `FUN_00483E30(0, 0x10000, 0, 1000, 1000, 1000)` do at this call site, and what visible presentation state does it feed?
4. Where are those two mechanisms routed in current Rust?
5. How should transaction 3 classify `Cell+0x30 = 0` and wall-owner reconstruction ordering without taking ownership of unrelated generic triggers, lighting, save/restore, or walls?

### Evidence needed to mark the research COMPLETE

- Live `gamemd.exe` decompile plus assembly for the two-pass clear/restamp and the exact per-cell order.
- Live predicate-chain proof down to event ids `0x19` and `0x1A`.
- Live active-consumer assembly, including register/scratch reuse after a horizontal scan.
- Live `Get_CellClass` and iterator evidence for sparse-diamond/shared-dummy behavior.
- Live `FUN_00483E30` and `FUN_00484180` evidence distinguishing ordinary cells from sentinel cells.
- Representative live draw consumers establishing the presentation effect.
- Retail YR data proving at least one shipped reachable event-25/26 use.
- Current Rust parser, runtime, lighting, dummy, and wall-owner ownership evidence.
- A handoff that separates what transaction 3 performs now from what it only sequences or routes.

### Non-goals

- Do not implement or fully design generic trigger execution.
- Do not decode unrelated trigger event/action kinds.
- Do not re-investigate the full LightConvert palette/blitter pipeline or superweapon lighting transitions.
- Do not invent a semantic name for `Cell+0x30`.
- Do not re-audit the already verified nearest-wall-owner distance algorithm beyond the ordering needed here.
- Do not revisit overlay Mark/Recalc identity, tiberium germination/value, terrain-Anim destruction/recreation, or `Cell+0x140 & 0x20000`; those are adjacent transaction-3 owners covered by their own reports.
- Do not treat OpenTS names or behavior as parity authority.

### Stop conditions

- Stop after the exact tail state/order, active consumers, retail reachability, current Rust route, and ownership split are proved.
- Record any remaining generic-trigger, LightConvert-output, or persistence question with its routed owner rather than expanding this slice.

## Executive Summary

The stale `BridgeZone_NS` / `BridgeZone_EW` interpretation is wrong. `Cell+0x140` bits `0x100000` and `0x200000` are derived acceleration marks for generic map-authored line-crossing trigger events. Event kind `0x19` marks a row and kind `0x1A` marks a column. `FootClass::PerCellProcess @ 0x004D85D0` consumes the marks on a cell-transition call and scans the corresponding row/column for every matching AttachedTag, offering event `0x19`/`0x1A` to each one. There is no direction or side-of-line comparison in this consumer: entry into a marked cell is the offer point.

The producer is deliberately two-pass. `InitCellAttributes` clears both high bits from every real cell before any tag stamps anything. Its second real-cell sweep never clears them again; it lets each qualifying tag OR its bit across the complete rectangular map bounds through `Get_CellClass`. Combining clear and stamp inside one per-cell loop would be wrong because a later visited cell would erase an earlier tag's line.

The shared dummy is not an inert implementation detail. Missing sparse-diamond slots all return the same dummy, so row/column stamps OR their bits into that one object. The first clear pass does not visit or clear the dummy. More importantly, the active consumer has a verified scratch-slot quirk: after scanning a horizontal row, it tests `0x200000` on the row's final `Get_CellClass` result, not on the mover's original cell. The vertical scan, if admitted, still uses the mover's original X. When the final row lookup is the shared dummy, the dummy's accumulated vertical mark controls this gate. A simplified “both bits on the mover means row then column” implementation is not exact.

The lighting call is also misdescribed by an older bridge-Z report. The six literal arguments are only initial/default values. With a null explicit converter, every ordinary cell calls `FUN_00484180`, which recomputes the current Scenario lighting, active point-light contributions, height terms, normalized RGB key, and three brightness values before `FUN_00483E30` stores them and selects/reuses a cached `LightConvertClass`. Only cell ids `(0,0)` and `(-1,-1)` take the neutral all-1000 branch. The resulting converter and scalar bundle feed terrain/TMP, overlay, TerrainClass, Techno SHP, Anim, and queued draw paths.

Current Rust already has a substantial presentation-owned analogue: `CellLightGrid`, normalized RGB profile caching, separate scalar variants, initial post-finalization rebuild, save-load rebuild, and fingerprint-driven deferred refresh. Transaction 3 must invalidate/route this at the native ancillary slot and prevent retained preview/load cache leakage, but transaction 20 owns semantic rendered-light equivalence. Current Rust parses CellTags/Tags/Events and builds a structural graph, but `TriggerRuntime` does not support event 25/26 or movement offers; it polls unsupported kinds to false. The real/dummy cell bridge-flag model masks to `0x1180` and cannot represent `0x300000`. Generic trigger ownership must close that behavior.

`Cell+0x30` is zeroed here, in the constructor, and in Resize relocation state; `CellClass::Load` also swizzle-remaps it as a persisted pointer slot. No live runtime producer/consumer beyond those lifecycle operations is proved. It remains dormant/unknown and belongs to transaction 21/OQ-19, not bridge state. Wall-owner reconstruction is active and already represented in Rust. Native invokes it only after the current cell's Recalc and current-overlay `Wall` recheck. Transaction 3 must reuse the existing Rust owner after the final-current-identity/Recalc-equivalent barrier, not add a duplicate wall algorithm.

## Bounded Mechanism Inventory

| Candidate mechanism | Native state/action | Active YR classification | Current owner | This slice's disposition |
|---|---|---|---|---|
| Derived horizontal line mark | `Cell+0x140 |= 0x100000` | Conditional on AttachedTag event `0x19`; active consumer | Generic trigger subsystem | Verified non-bridge mechanism; transaction 3 exposes only the ordered slot |
| Derived vertical line mark | `Cell+0x140 |= 0x200000` | Conditional on AttachedTag event `0x1A`; shipped retail use proved | Generic trigger subsystem | Verified non-bridge mechanism; exact consumer/dummy quirk recorded |
| Shared dummy line marks | same two bits on `0x00ABDC50` | Conditional on a stamped line traversing an unallocated map-bounds coordinate | Generic trigger/dummy integration | Must not be discarded by an exact future implementation |
| Cell light bundle refresh | `Cell+0x34`, `+0x104..+0x114` | Unconditional per real cell; visible output | Transaction 20 / app presentation lighting | Transaction 3 invalidates/routes only |
| Opaque pointer-slot zero | `Cell+0x30 = 0` | Unconditional write; semantic liveness unknown/dormant | Transaction 21 / OQ-19 | Sequence slot only; no invented field |
| Wall-owner reconstruction | `Cell+0x50` after Recalc/current-wall test | Conditional on current wall overlay | Existing GSI-04.07 overlay/wall owner | Reuse after final Recalc-equivalent barrier |
| `Cell+0x140 & 0x20000` clear | adjacent per-cell latch clear | Active | Transaction-3 terrain-Anim owner | Explicitly excluded from this report's semantic audit |
| Tiberium value/germination | adjacent argument-specific action | Active/conditional | Authored tiberium report/transaction 3 | Ordering boundary only |

## Active Fresh-Load Position

`ScenarioClass::Full_Init @ 0x00686B20` is an active retail caller. Its verified order around this tail is:

1. map section and IsoMapPack read;
2. Tubes read;
3. OverlayPack read;
4. an earlier cell Recalc sweep;
5. Terrain and then Techno/Building map sections;
6. `BuildingClass__ReadFromINI` and later object setup;
7. `_DAT_0087F91C = MapClass::InitCellAttributes(0)`;
8. beacon art, `ScenarioClass::Post_Map_Init`, deferred finalization drain, and final radar refresh.

Evidence: live decompile of `ScenarioClass::Full_Init @ 0x00686B20`; direct call to `0x00568BB0` after the Building read/setup cone. This is not a TS-only or editor-only helper. The two tag-line arms are content-conditional, while the clear, opaque zero, light refresh, and Recalc positions execute for every real cell in this call.

## Verified Binary Findings

### 1. The line marks use a global clear pass before any restamp

Active in YR: Yes on every `InitCellAttributes` call.

Live assembly in `MapClass::InitCellAttributes @ 0x00568BB0`:

- `0x00568BFD..0x00568C2D`: initializes the `MapClass` cell iterator.
- `0x00568C36`: loads mask `0xFFCFFFFF`.
- `0x00568C3B..0x00568C45`: applies `Cell+0x140 &= 0xFFCFFFFF`.
- `0x00568C4B`: advances with `MapClass::CellIterator_Next @ 0x00578290`.
- `0x00568C54..0x00568C84`: reinitializes the iterator for a distinct second sweep.

`0xFFCFFFFF == ~0x00300000`, so exactly `0x100000|0x200000` are cleared and every other flag bit is preserved. The iterator returns the allocated real map diamond and stops at its terminator; it does not return the shared dummy.

Required invariant:

```text
pass 1: for every real cell, clear both derived line bits
pass 2: for every real cell, run the remaining tail and allow tags to stamp any row/column
```

Do not transform this into `clear-this-cell; stamp-from-this-cell` in one sweep. A tag encountered early can stamp a cell encountered late; a combined later clear would erase the mark.

### 2. Horizontal and vertical predicates inspect the actual linked event graph

Active in YR: Conditional on an AttachedTag.

The complete live predicate chain is:

| Layer | Horizontal | Vertical | Exact check |
|---|---|---|---|
| `TagClass` wrapper | `FUN_006E5320` | `FUN_006E5300` | `Tag+0x24 != NULL`, then TagType helper |
| `TagTypeClass` trigger chain | `FUN_006E6250` | `FUN_006E6280` | starts at `TagType+0xA0`, advances entries through `entry+0xA8` |
| trigger event chain | `FUN_00726F80` | `FUN_00726F50` | starts at `entry+0xAC`, advances through `event+0x28`, tests `event+0x2C` |
| event id | `0x19` | `0x1A` | returns true on the first matching event anywhere in the linked chain |

Evidence: live decompiles of all six functions. The result is based on the parsed runtime TagType/Trigger/Event graph; it is not inferred from a tag name or map key.

### 3. Producer precedence is horizontal first and mutually exclusive per AttachedTag visit

Active in YR: Conditional.

The second sweep obtains `AttachedTag` from `Cell+0x3C` at `0x00568CC4`. It then performs:

1. `FUN_006E5320` at `0x00568CDD`;
2. if true, the horizontal row loop at `0x00568CE6..0x00568D54`, followed by an unconditional jump to the rest of the cell tail;
3. only if false, `FUN_006E5300` at `0x00568D5B`;
4. if true, the vertical column loop at `0x00568D64..0x00568DCA`.

Therefore a single AttachedTag whose linked graph contains both kinds contributes a row only. Its `0x1A` predicate is never asked during that visit. This is producer precedence, not a general prohibition on the same vertical event ever firing: a later consumer scan can still encounter that dual-event tag if some admitted vertical scan reaches its column.

### 4. Stamps span the rectangular map bounds through shared-dummy lookup

Active in YR: Conditional.

Horizontal loop:

```text
for i in 0 .. g_nMapCellArrayBoundsWidth:
    coord = (MapClass+0x124 + i, attached_cell.y)
    GetCell semantics
    result.Cell+0x140 |= 0x100000
```

Assembly: `0x00568CE6..0x00568D54`; width `0x0087F914`, left `MapClass+0x124`, fixed Y from `Cell+0x26`.

Vertical loop:

```text
for i in 0 .. g_nMapCellArrayBoundsHeight:
    coord = (attached_cell.x, MapClass+0x128 + i)
    GetCell semantics
    result.Cell+0x140 |= 0x200000
```

Assembly: `0x00568D64..0x00568DCA`; height `0x0087F918`, top `MapClass+0x128`, fixed X from `Cell+0x24`.

These are map-bounds rectangle rows/columns, not a loop restricted to allocated diamond cells. Zero or negative width/height executes no writes for that arm.

`MapClass::Get_CellClass @ 0x005657A0` computes signed `index = y*0x200+x`, accepts only `0..0x3FFFF`, reads the fixed sparse table, and returns shared dummy `0x00ABDC50` on an out-of-range or null slot after writing the requested coordinate to dummy `+0x24`. The inlined producer lookup has the same bound/null/dummy path at `0x00568D16..0x00568D35` and `0x00568D8B..0x00568DAF`.

Consequences:

- every sparse-diamond miss in every stamped line addresses the same dummy object;
- a missed horizontal coordinate ORs `0x100000` into the dummy;
- a missed vertical coordinate ORs `0x200000` into the dummy;
- later misses retain the accumulated union; only dummy `+0x24` changes per miss;
- the first real-cell clear pass never clears those dummy bits;
- ordinary Resize reconstructs the dummy with constructor-default low flags before a fresh scenario, but a repeated `InitCellAttributes` without that reconstruction does not itself neutralize them.

### 5. The active consumer is FootClass cell-transition processing

Active in YR: Yes for FootClass-derived movers; line work is conditional on marks/tags.

`FootClass::PerCellProcess @ 0x004D85D0` enters this body only when `param_2 == 2`. The relevant live assembly is:

- `0x004D8997`: resolves the mover's current cell.
- `0x004D89AC`: initially saves that cell pointer in stack scratch `[ESP+0x14]`.
- `0x004D8A70`: tests current `Cell+0x140 & 0x100000`.
- `0x004D8A7C..0x004D8AE7`: scans the same rectangular width at the mover's current Y.
- `0x004D8AB0..0x004D8ADA`: for every result with AttachedTag and true `FUN_006E5320`, calls `TagClass::ProcessTriggerEvent(0x19, mover, mover_cell, ...)`.
- `0x004D8AED`: tests `Cell+0x140 & 0x200000` on the pointer restored after the row loop; the scratch quirk is detailed below.
- `0x004D8AF9..0x004D8B64`: scans the rectangular height at the mover's original X.
- `0x004D8B2D..0x004D8B57`: for every result with AttachedTag and true `FUN_006E5300`, calls `TagClass::ProcessTriggerEvent(0x1A, mover, mover_cell, ...)`.

The consumer scans every coordinate in order and offers every matching tag; it does not stop at the first. There is no side-of-line, previous-side, direction, or geometric segment-crossing test in these loops. The observable offer occurs because a mover reached a cell during `PerCellProcess(param_2=2)`.

`TagClass::ProcessTriggerEvent @ 0x006E53A0` is the active tag processor. It rejects editor/inactive/reentrant states, walks the tag's runtime trigger-action entries, evaluates conditions, executes admitted actions, and handles cleanup/deferred finalization. `TriggerCondition::Evaluate @ 0x0071E940` groups `0x19` and `0x1A` with event kinds that require the offered kind to match and map-editor mode to be false; for these line events it also requires a non-null mover and applies the optional authored house filter against the mover's owner. This downstream trigger machinery, rather than the line-bit producer, owns trigger persistence, action execution, house semantics, and cleanup.

### 6. The post-horizontal vertical gate uses the final row lookup, not the mover cell

Active in YR: Yes whenever the horizontal branch executes with positive width. This is the most important cold-pass correction.

The assembly dataflow is exact:

1. `0x004D89AC`: `[ESP+0x14] = mover_current_cell` before either line test.
2. `0x004D8A81`: `EBP = 0` becomes the horizontal loop index.
3. `0x004D8AA4`: each row coordinate calls `MapClass::Get_CellClass`.
4. `0x004D8AAC`: `[ESP+0x14] = EAX` overwrites the scratch with that iteration's cell/dummy pointer.
5. `0x004D8AE9`: after the loop, `EBP = [ESP+0x14]` restores the final row lookup, not the original mover cell.
6. `0x004D8AED`: the vertical gate tests `[EBP+0x140] & 0x200000`.
7. `0x004D8B09`: the vertical loop reloads the mover's original X from `[ESP+0x1C]`, so admitted scanning still targets the mover's column.

Exact cases:

- If the mover cell lacks `0x100000`, no row scan occurs and the vertical test reads the mover cell normally.
- If the mover cell has `0x100000` but width is nonpositive, the loop does not overwrite the scratch and the vertical test still reads the mover cell.
- If the row loop runs, the vertical gate reads the last map-bounds row lookup `(left+width-1, mover_y)`.
- If that coordinate is unallocated, the gate reads the shared dummy's accumulated `0x200000`.
- The mover's own `0x200000` can therefore be ignored after a row scan, or a vertical scan can be admitted even when the mover's own bit is clear.
- An admitted vertical scan still examines only the mover's original X and fires only matching vertical tags found in that column.

This also refines dual-event behavior. Producer precedence means a dual-event tag stamps no column. Ordinarily another vertical stamp on the same column can make its vertical event discoverable. With the verified scratch/dummy quirk, an unrelated final-row/dummy vertical mark can also admit a scan of the mover's column while the mover is on a horizontal line; that scan may then discover a dual-event tag in that column even though the tag did not stamp it itself.

Do not implement the consumer as the intuitive rule “test both bits on the mover; if both are set, scan row then column.” That loses active native behavior.

### 7. Shipped YR retail data proves vertical-line reachability

Active in YR: Yes, at least in one official Yuri's Revenge campaign map.

Retail source: `mapsmd03.mix -> all01umd.map` from the installed active-retail YR data.

Verified rows:

```ini
[Basic]
Official=yes

[Map]
Size=0,0,65,120
LocalSize=12,5,47,100

[CellTags]
77056=0611EEAC

[Triggers]
0611BABC=Alliance,<none>,Kill_Harriers,0,1,1,1,0

[Events]
0611BABC=1,26,0,1

[Actions]
0611BABC=1,119,0,1,0,0,0,0,A

[Tags]
0611EEAC=0,Kill_Harriers,0611BABC
```

Event decimal `26` is native `0x1A`, the vertical-line event proved above. Packed CellTag `77056` resolves to `(56,77)` under the retail `y*1000+x` convention.

A conservative census extracted 184 unique named maps from the main retail map archives (`MAPS01.MIX`, `MAPS02.MIX`, `mapsmd03.mix`, `MULTI.MIX`, `multimd.mix`, `expandmd01.mix`) using the repository's prebuilt asset CLI. A first-condition pattern found this one direct event-25/26 row. This is a lower bound, not an absence proof: counted event rows can contain variable-width earlier records, unnamed/hash-only members were not claimed, and no claim is made that stock data lacks horizontal event 25. One official type-26 use is sufficient to reject “TS-only” or “dormant in YR.”

Current Rust cannot execute this shipped path: event kind 26 falls through `TriggerRuntime::evaluate_event` to false, and action 119 also falls through the current supported-action match. Action 119 semantics were not investigated here.

### 8. The lighting call recomputes ordinary cell lighting

Active in YR: Yes on every real cell in `InitCellAttributes`.

At `0x00568C98..0x00568CB9`, after `Cell+0x30 = 0`, native calls:

```text
ECX = current CellClass
FUN_00483E30(0, 0x10000, 0, 1000, 1000, 1000)
```

`FUN_00483E30 @ 0x00483E30` interprets the first explicit argument as an optional `LightConvertClass*`. Because it is null:

- cell id `(0,0)` or `(-1,-1)` takes the sentinel branch: cache/find neutral RGB `(1000,1000,1000)`, write `+0x104=0x10000`, `+0x108=0`, and write `1000` to all six shorts `+0x10A..+0x114`;
- every other cell passes the literal arguments by address to `FUN_00484180`, which overwrites them with computed outputs;
- an existing `Cell+0x34` converter is retained only when its normalized RGB key at converter `+0x198/+0x19C/+0x1A0` matches the newly computed key;
- a mismatched active converter has its reference count decremented and `Cell+0x34` cleared;
- `FUN_00544E70` finds or creates the converter for the new key;
- `FUN_00483E30` stores `+0x104`, `+0x108`, three brightness shorts `+0x10A/+0x10C/+0x10E`, and RGB-key shorts `+0x110/+0x112/+0x114`.

`FUN_00484180 @ 0x00484180` independently proves the ordinary calculation:

- sentinel check only for `(0,0)` and `(-1,-1)`;
- base ambient/R/G/B from current `ScenarioClass` lighting fields;
- every active/detail-admitted `LightSourceClass` in the global source array contributes signed intensity and R/G/B by radial linear falloff in lepton space;
- current cell ground height and level feed top/common and level-plus-four alternate brightness calculations under the normal/Lightning/Dominator profile gates;
- RGB normalization/clamping produces the converter key and fixed-point scale;
- scalar outputs are clamped into the native 0..2000 range after the verified arithmetic.

Therefore the call-site literals are defaults/scratch inputs, not the ordinary-cell result. Calling this slot a “neutral-light reset” is materially misleading.

### 9. Presentation effect ledger

This is a visual-state computation, but no new art asset is selected here. It chooses a per-cell palette converter/profile and scalar intensity inputs used by existing art draw paths.

| Stage | Native state/consumer | Exact effect | Rust route |
|---|---|---|---|
| Per-cell recompute | `FUN_00483E30 -> FUN_00484180 -> FUN_00544E70` | chooses/reuses RGB-keyed `LightConvertClass`; stores independent brightness variants | `CellLightGrid`, `LightProfileCache`, `build_cell_light_grid_from_heights_and_units_with_detail`, `accumulate_point_lights` |
| Ground TMP/tile | `CellOverlay_TileDraw @ 0x00480350` | uses `Cell+0x34` converter and `+0x10C` scalar | `render/terrain_instances.rs` calls `terrain_tile_tint_at` |
| Overlay body/bridge | `CellClass::DrawOverlay_Body @ 0x0047F6A0` | selects `+0x10A`, `+0x10C`, or `+0x10E` by branch | overlay instances use `overlay_tint_at`; bridge bodies use `bridge_body_tint_at` |
| Terrain object | `TerrainClass::Draw_It @ 0x0071C250` | normally `+0x10C`, special branch `+0x10A` | `terrain_object_tint_for_type` preserves two scalar branches |
| Techno SHP | `TechnoClass_DrawSHP @ 0x00705E00` | converter plus cell brightness affects unit/building/aircraft pixels | unit/SHP instances use category-specific accessors |
| Anim/queued draw | `AnimClass::DrawIt @ 0x00423200`, `FUN_004D1890` | cell-palette and brightness inputs affect animation/queued pixels | `anim_tint_at` in overlay/presentation instances |

Visible result: map-authored ambient/RGB/ground/level values and live light sources tint and brighten/darken terrain, overlays, units, buildings, aircraft, terrain objects, and many animations by cell. This is not bridge Z topology and does not alter simulation passability.

### 10. `Cell+0x30 = 0` is an active lifecycle write to a dormant/unknown pointer slot

Active in YR: The zero write is unconditional for every real cell in this pass; a live gameplay meaning is unproved.

Evidence:

- `CellClass::Constructor @ 0x0047BBF0` writes `param_1[0xC] = 0`, exactly `Cell+0x30 = NULL`.
- `MapClass::InitCellAttributes @ 0x00568CB2` writes the same zero immediately before lighting.
- `CellClass::Load @ 0x004839F0` calls `SwizzleManagerClass::Swizzle` on `Cell+0x30` (`param+0xC`) alongside other object pointers, proving the serialized payload is pointer-shaped rather than numeric scratch.
- `MapClass::Resize @ 0x00565C10` copies/restores the slot in cell relocation state and zeros it alongside `Cell+0x2C` in the temporary relocation copy.
- The prior exhaustive cell-layout study ([CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md), `LIVE-0610`) found no runtime writer beyond lifecycle/reset paths and classified the role dormant/unknown.

This slice found no new reader or semantic producer. Exact classification:

- the write and persistence mechanics are real;
- the pointed-to role is UNKNOWN;
- stock ordinary runtime liveness is DORMANT/UNPROVED;
- it is not generic scratch, bridge topology, a line-trigger pointer, a LightConvert pointer (`+0x34` owns that), or wall ownership (`+0x50` owns that);
- current Rust should not add an invented bridge field for it in transaction 3.

Transaction 21/OQ-19 owns any later persistence/restore decision or semantic expansion.

### 11. Wall-owner reconstruction uses the current post-Recalc wall

Active in YR: Conditional on a current wall overlay.

Exact per-cell order in `InitCellAttributes`:

1. argument-specific tiberium value/germination work;
2. `CellClass::RecalcAttributes(current, -1)` at `0x00568DF4`;
3. reload current `Cell+0x44` overlay id at `0x00568DF9`;
4. reject `-1`;
5. resolve current `OverlayTypeClass` and test `Wall` at `+0x2A8` (`0x00568E01..0x00568E12`);
6. only then call `CellClass::ReconstructWallOwnerFromNearestBuilding @ 0x0047D210` at `0x00568E16`.

The helper rechecks current wall status, initializes owner `Cell+0x50` to `-1`, scans the BuildingClass array in allocation order, accepts alive + marked/on-map + HouseType `WallOwner` buildings, computes native foundation-adjusted distance, replaces only on strict `<`, and writes the winning building owner-house index. Those semantics are already covered by the existing wall-owner research and current Rust tests; this report relies on them only to classify the ordering.

The ordering requirement is final-current-identity, not necessarily an interleaved Rust per-cell call. The wall helper reads no other cell's Recalc result, so a global Rust pass after all current-cell Recalcs is output-equivalent if buildings and current overlay identities are fixed. Reuse that existing global owner; do not run it from raw OverlayPack identity, before validation/Recalc, or through a second competing implementation.

## OpenTS Lead Check and TS-Legacy Filter

OpenTS was used only to locate likely names and relationships:

- `C:\Users\enok\Documents\OpenTS\code\tag.cpp`, `tagtype.cpp`, and `trigtype.cpp` suggested the `Is_Cross_Horizontal` / `Is_Cross_Vertical` predicate chain.
- `code\tevent.cpp:97-98` labels events 25/26 “Crosses Horizontal Line...” and “Crosses Vertical Line...”.
- `manual\content\scripting\events\25.md` and `26.md` suggested rectangular line scans, same-tag horizontal precedence, and the post-horizontal vertical-gate scratch quirk.
- `code\cell.cpp` `CellClass::Init_Drawer` suggested that null-converter inputs are recomputed for ordinary cells and neutral only for sentinel ids.

Every material claim above was then rederived from active `gamemd.exe` assembly/decompile and, for liveness, YR retail data. The TS-derived source was not used to fill an unverified binary gap. The official `all01umd.map` event-26 record independently proves that the vertical-line mechanism is active YR content, not a retained TS-only branch.

## Current Rust Surface and Delta

### Generic trigger data and runtime

Current parsed/static surfaces:

- `src/map/cell_tags.rs:11-40`: parses `[CellTags]` into `(u16,u16) -> tag id` using `y*1000+x`.
- `src/map/tags.rs`: preserves Tag records.
- `src/map/events.rs`: parses counted event records into `EventCondition { kind, params }`.
- `src/map/trigger_graph.rs:51-128`: structurally resolves cell tags -> tags -> triggers and retains each trigger's `tagged_cells`.

Current runtime gap:

- `src/sim/trigger_runtime.rs:29-47` declares supported events; 25/26 are absent.
- `src/sim/trigger_runtime.rs:236-285` evaluates supported event kinds and returns false for every other kind.
- `TriggerRuntime::advance_at_frame` is polling-oriented and receives no mover cell-entry offer.
- `src/sim/world/mod.rs:321-329` `TriggerInputs` carries graph/triggers/events/actions/waypoints/rules, but no derived row/column line-mask state or cell-entry event queue.
- no movement/PerCellProcess-equivalent call site offers event 25/26.
- action 119 is also unsupported, so the shipped `Kill_Harriers` record remains open beyond this event mechanism.

Current dummy/state boundary:

- `src/map/resolved_terrain.rs:423-460` snapshots and stores only `CellClass+0x140 & 0x1180` bridge flags.
- `SharedCellDummy` accessors at `src/map/resolved_terrain.rs:784-860` mask to that bridge subset.
- `Simulation` owns the bridge-only shared dummy and real-cell `0x1180` authority at `src/sim/world/mod.rs:805-811`.

Therefore current Rust cannot represent native line marks on real cells or the dummy. Generic trigger work must either add separate derived line-mask authority or deliberately widen a generic cell/dummy state owner; transaction 3 must not smuggle `0x300000` into `BridgeFacts` or the bridge-only `0x1180` mask.

### Lighting route

Current Rust is substantially aligned in architecture:

- `src/map/lighting.rs:143-199`: RGB-keyed `LightProfileCache`.
- `src/map/lighting.rs:202-272`: `CellLight` stores normalized key, raw RGB, scale16, additive intensity, raw and clamped top/common/bottom scalars.
- `src/map/lighting.rs:642-671`: builds every resolved cell, special-casing `(0,0)` to neutral and otherwise applying scenario profile + height.
- `src/map/lighting.rs:952-1016`: signed point-light accumulation before final normalization.
- `src/map/lighting.rs:1172-1187`: neutral sentinel bundle.
- `src/app/loading/init.rs:1290-1351`: derives current committed Scenario profile plus live building/radiation sources.
- `src/app/loading/init.rs:1355-1380`: builds the presentation grid from final resolved terrain and point lights.
- `src/app/loading/init.rs:2374-2382` then `2468-2474`: sim finalization precedes the initial background-loader lighting build.
- `src/app/loading/transitions.rs:224-250`: foreground handoff rederives the grid at the selected detail mask and records the source fingerprint.
- `src/app/match_runtime/sim_tick.rs:1473-1557`: changed lighting fingerprints schedule a deferred all-gathered-before-commit refresh.
- `src/app/input/dispatch.rs:1798-1812`: save-load rebuilds transient lighting and invalidates pending/fingerprint state.

The grid is presentation/app-derived rather than simulation/hash authority, matching the native role of the per-cell converter/scalar bundle. Exact semantic/pixel equivalence remains transaction 20 because GPU tint/profile use is not byte-identical proof of `LightConvertClass` palette conversion, and because transaction 20 owns stale-preview-cache tests. Transaction 3's action is only to execute one invalidation/routing boundary in the correct post-object tail slot and preserve final recomputation from committed state.

### Wall owner

- `src/sim/runtime.rs:737-825` owns shared post-funnel scenario finalization.
- `src/sim/runtime.rs:757-784` gathers existing spawned building candidates and calls the existing overlay-grid wall-owner owner before installing the grid.
- `src/sim/overlay_grid.rs:368-414` implements current-wall filtering, candidate gates, strict improvement, and wall-owner storage.
- focused tests at `src/sim/overlay_grid.rs:1952-2018` cover candidate filtering and strict first-on-tie behavior.

The current implementation should be moved/reused only as needed to sit after transaction 3's final-current-identity/Recalc-equivalent barrier. No new wall-owner algorithm or raw-pack pass is justified.

### `Cell+0x30`

No current Rust field was found, which is correct for transaction 3. The proven native zero slot belongs in the finalization trace/ordering ledger only until transaction 21 resolves whether any wider save/restore owner needs it.

## Implementation Handoff

### What transaction 3 must perform now

1. **Expose the exact ordered ancillary seam.** The finalization trace/contract must contain these positions in this order:

   ```text
   A. global real-cell line-mask clear slot (before any restamp)
   B. for each real cell in native sweep order:
      1. opaque +0x30-zero slot
      2. cell-light invalidation/recompute-routing slot
      3. existing 0x20000 latch clear owner
      4. tag-line restamp slot (0x19 predicate before 0x1A)
      5. argument-specific value/germination work
      6. Recalc/current-identity finalization
      7. current-wall owner reconstruction
   ```

   Transaction 3 may represent foreign slots as typed trace/routing callbacks; it must not claim their unimplemented semantics as closed.

2. **Execute one lighting cache invalidation at slot B2.** Any map-load/RMG-preview `CellLightGrid`, pending refresh, applied profile/source list, and view fingerprint that could survive into the accepted scenario must be invalidated exactly once at this boundary. Actual visible grid construction may remain batched at the existing post-finalization app handoff, where it reads committed Scenario/building/radiation state. Do not publish a partially recomputed grid.

3. **Reuse the existing wall owner after the final-current-identity barrier.** The current `OverlayGrid::reconstruct_map_wall_owners` remains the single semantic owner. Run it only after the transaction-3 Recalc-equivalent has finalized every current overlay identity; a post-all-Recalc pass is acceptable because the helper has no cross-cell Recalc dependency. Do not reconstruct from raw OverlayPack identity and do not add a duplicate per-cell owner.

4. **Prove negative non-ownership.** Transaction-3 tests must assert that ancillary line slots cannot write `BridgeFacts`, bridge topology, zones, pathing, the bridge-only `SharedCellDummy & 0x1180`, or a newly invented `+0x30` field. The stale `BridgeZone_NS/EW` label must not survive in new code or tests.

5. **Keep the mechanism open where routed owners are absent.** A trace slot is not generic trigger parity; an invalidation call is not LightConvert output parity; an opaque zero event is not persisted `+0x30` parity.

Suggested transaction-3 acceptance tests:

- `init_cell_attributes_ancillary_slots_preserve_native_order`
- `init_cell_attributes_line_slots_never_mutate_bridge_facts_or_bridge_dummy_mask`
- `post_object_light_slot_invalidates_preview_grid_once_before_final_rebuild`
- `wall_owner_reconstruction_reuses_current_overlay_after_final_recalc`

### What transaction 3 only sequences/routes

#### Generic trigger owner

Must later implement and validate:

- real-cell two-pass clear/restamp state for event 25/26;
- map-bounds rectangle traversal and shared-dummy writes;
- AttachedTag graph predicates with horizontal producer precedence;
- `FootClass::PerCellProcess` cell-entry offers;
- every matching tag in scan order;
- the final-row/dummy vertical-gate quirk while retaining mover-original X;
- trigger house/persistence/action semantics and save/hash authority as appropriate;
- shipped `all01umd.map` vertical-event behavior, including separately owned action 119.

The existing bridge-only `SharedCellDummy` mask is not sufficient. This owner must choose a generic derived-state representation; transaction 3 must not choose it on its behalf.

Suggested generic-trigger tests:

- `line_trigger_build_clears_all_real_cells_before_any_restamp`
- `dual_event_tag_stamps_horizontal_only`
- `line_trigger_bounds_misses_accumulate_on_shared_dummy`
- `horizontal_scan_vertical_gate_reads_final_row_lookup_but_scans_original_x`
- `line_entry_fires_every_matching_tag_in_map_bounds_order`
- `retail_all01umd_vertical_line_event_reaches_kill_harriers_trigger`

#### Transaction 20 lighting owner

Must later prove:

- semantic equality of ordinary-cell `FUN_00484180` outputs, cache keys, scalar variants, detail masks, and active source gates;
- neutral handling only for `(0,0)`/`(-1,-1)` or exact Rust-equivalent missing/dummy queries;
- render consumers use the correct scalar variant, not one universal tint;
- no retained RMG-preview/load cache contaminates the accepted map;
- sufficiently exact `LightConvertClass` palette/pixel output for the project's presentation parity bar.

Suggested transaction-20 correction test:

- `initcell_null_converter_recomputes_ordinary_cell_but_keeps_sentinels_neutral`

#### Transaction 21 / OQ-19

Owns any decision to retain, serialize, restore, swizzle-equate, or otherwise model the opaque `Cell+0x30` pointer slot. Until it proves a consumer, transaction 3 records only the zero position and adds no field.

## Negative Facts / Do Not Do

- Do not name `0x100000`/`0x200000` bridge zones. Their proved active consumer is generic trigger-line dispatch.
- Do not map either bit into bridge layer, connectivity, passability, zones, or `BridgeFacts`.
- Do not stamp only allocated cells. Native walks the enclosing map-bounds rectangle through shared-dummy lookup.
- Do not clear a cell's line bits immediately before processing its own tag in the second sweep. Native clears all real cells first.
- Do not clear the shared dummy in that first pass. Native does not visit it.
- Do not test the mover's vertical bit after a horizontal scan. Native tests the final row lookup/dummy.
- Do not change the vertical scan's X to the final row coordinate. Native retains the mover's original X.
- Do not assume a dual-event tag can never fire vertically. It stamps no column itself, but an admitted scan can still encounter it.
- Do not call the light slot a neutral reset for ordinary cells. It recomputes live Scenario/source/height lighting.
- Do not apply the six literal inputs directly to ordinary cells; `FUN_00484180` overwrites them.
- Do not collapse the three native brightness fields into one without transaction-20 consumer proof.
- Do not add `Cell+0x30` as bridge state or numeric scratch.
- Do not reconstruct wall owner from pre-Recalc/raw-pack identity or introduce a second wall owner.

## Stale-Document Corrections

### [FUN_00483E30_BRIDGE_Z_AT_MAP_LOAD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/FUN_00483E30_BRIDGE_Z_AT_MAP_LOAD_GHIDRA_REPORT.md)

That report's central ordinary-cell claim is superseded. It says the map-load call writes literal `1000` to `Cell+0x10E` and that height-plus-four appears only in a later superweapon path. Live decompile of `FUN_00483E30 @ 0x00483E30` shows why that reading is incomplete: for an ordinary cell with null explicit converter, the function passes `param_3..param_7` by address into `FUN_00484180`, which overwrites the defaults before the final stores. `FUN_00484180 @ 0x00484180` computes a level-plus-four alternate value in the ordinary no-superweapon branch as part of the same call. Literal all-1000 storage applies only to sentinel ids `(0,0)` and `(-1,-1)`.

Replacement wording:

> `MapClass::InitCellAttributes` supplies neutral defaults to `FUN_00483E30`, but ordinary cells immediately recompute those values through `FUN_00484180` from current Scenario lighting, active light sources, and cell level. Only `(0,0)` and `(-1,-1)` keep the neutral bundle. `Cell+0x10E` is the alternate/bridge-height brightness output, not a map-load constant for ordinary cells.

### Prior simple line-consumer wording

Any wording that says “if both bits are set, FootClass scans the row then the column” is incomplete. Replacement:

> FootClass tests the mover's horizontal bit first. If the row loop runs, its scratch cell pointer is overwritten by each row lookup and the subsequent vertical gate tests the final row result—possibly the shared dummy—while the vertical scan still uses the mover's original X.

### Transaction-3 design wording

Use “map-bounds rectangle row/column,” not “allocated playfield row/column.” Use “cell-light invalidation/recompute-routing slot,” not “neutral-light reset.” State that transaction 3 executes invalidation and ordering only, while generic trigger behavior, transaction-20 light output, and transaction-21 opaque persistence remain open.

## Adversarial Checks

1. **What if one AttachedTag contains both events?** Horizontal predicate wins in the producer; only the row is stamped. The vertical event can still be reached by a separately admitted vertical scan.
2. **What if two independent tags produce an intersecting row and column?** Both real bits may be stamped on the intersection, but the consumer's vertical gate after the row scan reads the final row lookup, not necessarily that intersection.
3. **What if a stamp traverses sparse-diamond holes?** All misses mutate one shared dummy; bits accumulate and can later control the post-horizontal vertical gate.
4. **What if bounds width/height is zero or negative?** That producer/consumer loop makes no iterations. A non-running row loop leaves the original cell scratch intact.
5. **What if an ordinary call uses the six neutral-looking constants?** `FUN_00484180` overwrites them; only sentinel ids remain neutral.
6. **What if no light sources are active?** Scenario ambient/RGB/ground/level and cell height still produce the ordinary bundle.
7. **What if Recalc invalidates or changes the current overlay?** Wall-owner reconstruction reloads and tests the post-Recalc current overlay; raw/pre-Recalc wall identity is insufficient.
8. **What if current Rust simply widens its bridge dummy mask?** That would conflate a generic trigger owner with bridge authority and still would not implement the Foot scratch quirk; it is rejected for transaction 3.

## Coverage Ledger

| Surface | Coverage | Evidence | Remaining owner |
|---|---|---|---|
| `MapClass::InitCellAttributes @ 0x00568BB0` non-overlay tail | Complete for requested writes/order | full decompile + full relevant assembly | none for research |
| real-cell clear iterator | Complete | `0x00568BFD..0x00568C84`, `CellIterator_Next @ 0x00578290` | none |
| horizontal predicate chain | Complete | `0x006E5320 -> 0x006E6250 -> 0x00726F80` | generic trigger implementation |
| vertical predicate chain | Complete | `0x006E5300 -> 0x006E6280 -> 0x00726F50` | generic trigger implementation |
| map-bounds stamp/dummy | Complete | primary assembly + `Get_CellClass @ 0x005657A0` | generic trigger implementation |
| active consumer row/column loops | Complete | `FootClass::PerCellProcess @ 0x004D85D0`, assembly `0x004D8A70..0x004D8B66` | generic trigger implementation |
| post-horizontal scratch quirk | Complete; cold rechecked | assembly writes/restores/tests at `0x004D8AAC`, `0x004D8AE9`, `0x004D8AED` | generic trigger implementation |
| downstream line event matching | Complete for event-kind/mover/house boundary | `TagClass::ProcessTriggerEvent @ 0x006E53A0`, `TriggerCondition::Evaluate @ 0x0071E940` | full trigger/action owner |
| stock YR liveness | Sufficient and direct | `mapsmd03.mix:all01umd.map` official event 26 | none for liveness; action 119 open |
| `FUN_00483E30` ordinary/sentinel split | Complete | live decompile `0x00483E30` | transaction 20 output parity |
| cell light formula boundary | Complete for requested presentation effect | live decompile `0x00484180`; representative draw xrefs | transaction 20 pixels/cache |
| current Rust lighting route | Complete source audit | `lighting.rs`, loading transition, tick refresh, save-load rebuild | transaction 3 seam + transaction 20 semantics |
| `Cell+0x30` classification | Complete to bounded stop: pointer-shaped, lifecycle-zeroed, semantic role unknown | ctor/load/resize/primary + prior exhaustive cell study | transaction 21/OQ-19 |
| wall-owner ordering | Complete | primary call order + `0x0047D210` + current Rust owner | transaction 3 reuse/order only |
| unrelated trigger kinds/actions | Excluded | not needed | generic trigger backlog |
| full LightConvert pixel blitters | Excluded | representative consumers sufficient | transaction 20 |

## Open Questions Final State

- OQ-01: Which bits are cleared? **RESOLVED:** exactly `0x100000|0x200000` from real `Cell+0x140`.
- OQ-02: One pass or two? **RESOLVED:** a complete real-cell clear pass precedes a separate complete processing/restamp pass.
- OQ-03: What makes a horizontal tag? **RESOLVED:** any linked event kind `0x19` in the AttachedTag's TagType trigger/event graph.
- OQ-04: What makes a vertical tag? **RESOLVED:** any linked event kind `0x1A` in that graph.
- OQ-05: Both kinds on one tag? **RESOLVED:** horizontal producer precedence; no column stamp from that visit.
- OQ-06: What bounds are stamped/scanned? **RESOLVED:** full `g_nMapCellArrayBounds` rectangle with shared-dummy lookup.
- OQ-07: Does the first pass clear the dummy? **RESOLVED:** no.
- OQ-08: Who consumes the marks? **RESOLVED:** `FootClass::PerCellProcess(param_2=2)`.
- OQ-09: What does a line crossing mean here? **RESOLVED:** mover cell entry into an admitted marked cell; no direction/side comparison in the consumer.
- OQ-10: Does one scan stop after one tag? **RESOLVED:** no; every matching tag in row/column order is offered.
- OQ-11: If horizontal runs, where is vertical gated? **RESOLVED:** final horizontal-row lookup/dummy, not mover cell.
- OQ-12: Which X does the vertical scan use? **RESOLVED:** mover-original X.
- OQ-13: Is the mechanism active in retail YR? **RESOLVED:** yes; official `all01umd.map` uses event 26.
- OQ-14: Does current Rust parse the source records? **RESOLVED:** structurally yes for the sole event-26 row and CellTag/Tag graph.
- OQ-15: Does current Rust execute event 25/26? **RESOLVED:** no; unsupported events evaluate false and there is no mover-entry offer.
- OQ-16: Is the lighting call neutral for all cells? **RESOLVED:** no; only `(0,0)` and `(-1,-1)` are neutral.
- OQ-17: What does ordinary lighting include? **RESOLVED:** current Scenario profile, active/detail-admitted LightSources, cell height, normalization, converter-key selection, and scalar outputs.
- OQ-18: Is the output visible? **RESOLVED:** yes across terrain, overlays, TerrainClass, Techno SHP, Anim, and queued draw consumers.
- OQ-19: Where does current Rust route it? **RESOLVED:** transient presentation `CellLightGrid`, initial/final handoff rebuild, fingerprint refresh, save-load rebuild.
- OQ-20: Does transaction 3 own light semantics? **RESOLVED:** no; it owns one invalidation/routing slot, transaction 20 owns output parity.
- OQ-21: What is `Cell+0x30`? **DEFERRED to transaction 21/OQ-19:** persisted/swizzled pointer slot, lifecycle zeroed, semantic role and live consumer unknown. Category: requires different persistence/system context. No implementation is justified here.
- OQ-22: When is wall owner computed? **RESOLVED:** after the current cell Recalc and current-overlay Wall recheck.
- OQ-23: Must Rust duplicate the wall algorithm in the per-cell loop? **RESOLVED:** no; reuse the existing global owner after all final-current Recalcs.
- OQ-24: Does exact `LightConvertClass` pixel conversion already meet parity? **DEFERRED to transaction 20:** this slice proves routing/effect, not pixel equivalence.
- OQ-25: What does action 119 do? **DEFERRED to generic trigger/action ownership:** not required to establish event-26 liveness or this finalization contract.

Deferred questions are 3 of 25 and all cross the stated ownership/non-goal boundary; no target-scope behavior remains unverified.

## Cold Spot Checks and Zero-Add Pass

Cold spot check 1: re-read `FootClass::PerCellProcess` in raw assembly after the initial decompile. This overturned the tempting decompiler interpretation that the mover cell is restored after the row loop: `0x004D8AAC` overwrites the same stack slot restored at `0x004D8AE9`. The final report incorporates the corrected last-row/dummy vertical gate.

Cold spot check 2: re-decompiled `FUN_00483E30` and `FUN_00484180` independently after reading the older bridge-Z report. The explicit pass-by-address call proved that ordinary cells overwrite the six defaults and that only the two sentinel ids retain neutral values.

Zero-add pass: repeated the primary assembly, predicate-chain batch decompile, consumer assembly, current Rust search, and conservative retail map census after drafting the coverage ledger. No additional in-scope mechanism appeared. The only additions were the stale-doc corrections and explicit ownership split already integrated above.

## Ghidra Annotation Candidates (Report Only; Not Applied)

No Ghidra metadata was modified. High-confidence candidates for a later parent sync:

| Address | Proposed name | Basis |
|---|---|---|
| `0x006E5320` | `TagClass__HasHorizontalLineCrossingEvent` | receiver requires `Tag+0x24`, delegates to verified TagType graph predicate |
| `0x006E5300` | `TagClass__HasVerticalLineCrossingEvent` | symmetric event-`0x1A` predicate |
| `0x006E6250` | `TagTypeClass__HasHorizontalLineCrossingEvent` | walks TagType trigger entries, calls event-`0x19` chain helper |
| `0x006E6280` | `TagTypeClass__HasVerticalLineCrossingEvent` | walks TagType trigger entries, calls event-`0x1A` chain helper |
| `0x00726F80` | `TriggerActionEntry__HasHorizontalLineEvent` | walks `+0xAC` event chain for kind `0x19` |
| `0x00726F50` | `TriggerActionEntry__HasVerticalLineEvent` | walks `+0xAC` event chain for kind `0x1A` |
| `0x00483E30` | `CellClass__InitOrSetLightConvert` | explicit-converter or computed/cache-backed cell light bundle |

## Sources Consulted

### Live active binary

- `gamemd.exe` in the live Ghidra project:
  - `MapClass::InitCellAttributes @ 0x00568BB0`
  - `ScenarioClass::Full_Init @ 0x00686B20`
  - `MapClass::CellIterator_Next @ 0x00578290`
  - `MapClass::Get_CellClass @ 0x005657A0`
  - predicate chain `0x006E5320`, `0x006E5300`, `0x006E6250`, `0x006E6280`, `0x00726F80`, `0x00726F50`
  - `FootClass::PerCellProcess @ 0x004D85D0`
  - `TagClass::ProcessTriggerEvent @ 0x006E53A0`
  - `TriggerCondition::Evaluate @ 0x0071E940`
  - `FUN_00483E30 @ 0x00483E30`
  - `FUN_00484180 @ 0x00484180`
  - representative draw consumers listed in the presentation ledger
  - `CellClass::Constructor @ 0x0047BBF0`
  - `CellClass::Load @ 0x004839F0`
  - `MapClass::Resize @ 0x00565C10`
  - `CellClass::ReconstructWallOwnerFromNearestBuilding @ 0x0047D210`

### YR retail data

- Installed active-retail YR archives, especially `mapsmd03.mix -> all01umd.map`.
- Conservative 184-named-map census using the repository prebuilt `target/release/asset.exe`; no Cargo command was run.

### Current Rust

- `src/map/cell_tags.rs`
- `src/map/tags.rs`
- `src/map/events.rs`
- `src/map/trigger_graph.rs`
- `src/sim/trigger_runtime.rs`
- `src/sim/world/mod.rs`
- `src/map/resolved_terrain.rs`
- `src/map/lighting.rs`
- `src/app/loading/init.rs`
- `src/app/loading/transitions.rs`
- `src/app/match_runtime/sim_tick.rs`
- `src/app/input/dispatch.rs`
- `src/sim/runtime.rs`
- `src/sim/overlay_grid.rs`

### Existing research checked, corrected, or routed

- [docs/research/MAP_LIGHTCONVERT_CACHE_00483E30_00544E70_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAP_LIGHTCONVERT_CACHE_00483E30_00544E70_GHIDRA_REPORT.md)
- [docs/research/MAP_LIGHTING_CELL_COMPUTE_00484180_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAP_LIGHTING_CELL_COMPUTE_00484180_GHIDRA_REPORT.md)
- [docs/research/MAPCLASS_GHIDRA_REPORT_FOLLOWUP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_GHIDRA_REPORT_FOLLOWUP.md) (stale/partial; used only as a lead)
- [docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_MAPCLASS_ENGINE_SUBSTRATE_SERVICE_STUDY.md)
- [docs/research/REGULAR_OVERLAY_WALL_AUTOFILL_COMMIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/REGULAR_OVERLAY_WALL_AUTOFILL_COMMIT_GHIDRA_REPORT.md)
- [docs/research/bridges/01-assets-map-load-overlay/FUN_00483E30_BRIDGE_Z_AT_MAP_LOAD_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/FUN_00483E30_BRIDGE_Z_AT_MAP_LOAD_GHIDRA_REPORT.md) (ordinary-cell conclusion superseded above)

### OpenTS lead-only reference

- `C:\Users\enok\Documents\OpenTS\code\tag.cpp`
- `C:\Users\enok\Documents\OpenTS\code\tagtype.cpp`
- `C:\Users\enok\Documents\OpenTS\code\trigtype.cpp`
- `C:\Users\enok\Documents\OpenTS\code\tevent.cpp`
- `C:\Users\enok\Documents\OpenTS\code\cell.cpp`
- `C:\Users\enok\Documents\OpenTS\manual\content\scripting\events\25.md`
- `C:\Users\enok\Documents\OpenTS\manual\content\scripting\events\26.md`

## Final Status

**COMPLETE for the requested focused research slice.** The exact line-bit identity, producer precedence, map-bounds/shared-dummy writes, active FootClass consumer, last-row/dummy vertical-gate quirk, official YR reachability, ordinary-versus-sentinel lighting behavior, presentation effect, current Rust routing, `+0x30` classification, and post-Recalc current-wall ordering are all pinned. Transaction 3 may now close only its ordered ancillary seam, light invalidation, bridge non-ownership, and existing wall-owner reuse; generic line-trigger parity, transaction-20 light-output parity, transaction-21 `+0x30`, and action 119 remain explicitly open under their own owners.
