# OverlayPack Shared Dummy and Final Recalc Field Corridor

Date: 2026-08-31
Status: **COMPLETE for the bounded shared-dummy/final-Recalc field and consumer corridor**
System rows: GSI-04.13 primary; GSI-04.12 load-source and OverlayData ordering boundary; transaction-21 persistence explicitly deferred

Activity vocabulary is literal. **Active in YR: Yes** means an ordinary active retail path uses the behavior. **Active in YR: Conditional** means the active executable and retail declarations support it, but the authored low-trigger branch requires compatible map input and the prior installed-retail census found no trigger cell. **Active in YR: No** marks a disproved field, effect, or ownership claim. OpenTS is a navigation lead only.

## Executive verdict

- **Active in YR: Conditional.** The authored low-Mark corridor adds exactly three mutable facts to the persistent fallback object: packed coordinate `CellClass+0x24` (`x:i16,y:i16` in one dword), overlay identity `+0x44` (signed dword, `-1` means none), and overlay state `+0x11E` (byte). `Get_CellClass` stamps the coordinate; each low fixed/body write stores identity and state before calling Recalc.
- **Active in YR: Yes.** `CellClass::RecalcAttributes @ 0x0047D2B0` compares `this` with the fixed dummy `0x00ABDC50` before any helper or field access and jumps directly to the epilogue. Dummy Recalc is a total semantic no-op. It does **not** derive or mutate dummy Land, zone, cache, LAT tile, slope, level, elevation, animation flags, or state; the explicit low-Mark `+0x44/+0x11E` stores nevertheless remain observable.
- **Active in YR: Conditional.** Real-cell Recalc derives Land `+0xEC`, reduced zone `+0x4C`, LAT tile `+0x38`, sub-tile/slope/elevation support fields, and selected compact cache bytes from overlay identity and terrain/object inputs. Overlay state `+0x11E` is not a Recalc input; it is only zeroed if Recalc clears an invalid identity.
- **Active in YR: Conditional.** The consumed-once finalized overlay payload needs the post-Mark/post-OverlayData/post-first-sweep **real-cell identity and state vector only**. Land, zone, LAT, cache, slope/elevation, and bridge facts are live derived projection already owned by `ResolvedTerrainGrid`; duplicating them in the payload would create a second authority. The shared dummy is process state and must not be included in that real-cell payload.
- **Active in YR mismatch: Conditional.** Current Rust has one shared dummy identity, but its single `AtomicU64` stores only coordinate, level, slope, and modeled bridge flags. It cannot retain low-Mark overlay/state. Current `ResolvedTerrainGrid` does not run authored low Mark, while `OverlayGrid::from_native_overlay_packs` later decodes/filter/recalculates the raw packs a second time and therefore cannot consume procedural identities.

## Target question and stop condition

Prove the exact persistent fallback `CellClass` state and finalized real-cell state required by the authored OverlayPack low-Mark transaction, beginning at both Map lookup overloads, the fixed dummy, the four low fixed/body write spans, and the real/dummy split in `RecalcAttributes`, then close the directly relevant RecalcZone, compact cache, LAT, load-order, retail-data, current-Rust, and downstream-reader corridors.

COMPLETE required decompile plus disassembly proof for offset, width, signedness, guard order, and write sites; constructor/Resize lifetime proof; xref/reader closure for every implementation-relevant real output; explicit separation of payload, derived projection, and process dummy state; five adversarial questions; two cold spot-checks; an open-question drain; and a zero-additional-field pass. Those bounded conditions are satisfied. Native save/load serialization of the dummy is outside this transaction and remains open for transaction 21.

## Scope and exclusions

- **Active in YR: Yes.** In scope: `MapClass::Get_CellClass @ 0x005657A0`, sibling world-coordinate overload `0x00565730`, dummy `0x00ABDC50`, `CellClass::Constructor @ 0x0047BBF0`, the dummy reconstruction inside `MapClass::Resize @ 0x00565C10`, low procedural stores inside `OverlayClass::Mark @ 0x005FC570`, `CellClass::RecalcAttributes @ 0x0047D2B0`, `CellClass__ApplyLAT_and_SlopeFixup @ 0x0047CA80`, `CellClass::RecalcZoneType @ 0x00483C80`, compact-cache builders/readers, and the authored OverlayPack/OverlayData/global-Recalc ordering.
- **Active in YR: No (this slice).** Not in scope: re-deriving low geometry/RNG tables, high-bridge stamping, damage/repair algorithms, full pathfinding parity, unrelated Recalc object side effects, pixel parity, generated-map construction, or Rust implementation.
- **Active in YR: No (this slice).** Stream/snapshot serialization semantics are not inferred from fresh `Full_Init`. This report identifies the process-state boundary but does not decide whether native transaction 21 serializes any dummy field.
- **Active in YR: No.** No Ghidra metadata was changed. Labels and local names are treated as hypotheses; instruction widths, control flow, callers, and retail data decide.

## Primary evidence ledger

- **Active in YR: Yes.** Read-only Ghidra decompile and disassembly: `0x005657A0`, `0x00565730`, `0x0047BBF0`, `0x00565C10`, `0x005FC570`, `0x0047D2B0`, `0x0047CA80`, `0x00483C80`, `0x0056D3F0`, `0x0056C510`, `0x00581F90`, `0x005824A0`, `0x00584550`, and `0x00586990`. Load-bearing fields were checked in instructions rather than accepted from decompiler types.
- **Active in YR: Conditional.** Exact low-Mark stores were independently checked in both families: wood fixed/body `0x005FC976..0x005FC981` and `0x005FCB67..0x005FCB70`; concrete fixed/body `0x005FCDA4..0x005FCDAF` and `0x005FCF95..0x005FCF9E`.
- **Active in YR: Yes.** Downstream closure used read-only decompile/disassembly/xrefs for `ZoneMap__BuildZoneLevel @ 0x00581F90`, `ZoneMap__FloodFillScanline @ 0x005824A0`, `MapClass::GetZoneID @ 0x0056D230`, `Zone_precheck @ 0x0042C290`, `AStar_main_loop @ 0x00429E8A`, `PathfinderClass__UpdateHierarchicalEdges @ 0x0042CCD0`, radar/tile renderers, bridge tile classifiers, and low repair/damage readers.
- **Active in YR: Yes.** Retail declaration checks used `ini/rulesmd.ini` low bridge sections and `ini/artmd.ini` images. The trigger branch remains **Conditional** because activation requires an authored compatible endpoint trigger; this report relies on, but does not rerun, the prior 385-payload zero-trigger census.
- **Active in YR: No (authority).** `C:\Users\enok\Documents\OpenTS\code\map.cpp`, `cell.cpp`, and `overlay.cpp` supplied candidate function/field relationships only. Every material conclusion was independently checked in `gamemd.exe` and retail YR data.

## 1. Both lookup overloads and the persistent alias

### Packed-cell overload `0x005657A0`

- **Active in YR: Yes.** The function sign-extends both 16-bit coordinate components. It computes `linear = signed_y * 512 + signed_x`, then tests only `0 <= linear < 0x40000` and whether that fixed-table slot contains a non-null real `CellClass*`.
- **Active in YR: Yes.** This is not a per-axis rectangle check. A negative component can alias a real slot when the signed linear result is valid; for example `x=-510,y=2` produces `linear=514`. Rust must reproduce fixed-stride linear resolution before fallback.
- **Active in YR: Yes.** On a miss, the exact input dword is stored to dummy `0x00ABDC50+0x24 = 0x00ABDC74`, and the same object at `0x00ABDC50` is returned. A later miss overwrites only the coordinate unless its caller explicitly writes other fields.
- **Active in YR: Yes.** On a real hit, the dummy is neither stamped nor reset. Every miss throughout the pass aliases one persistent object; a fresh default object per lookup is observably wrong.
- **Active in YR: Conditional.** Low Mark forms working coordinates with 16-bit word arithmetic. Overflow wraps before the lookup sign-extends the words; widening and clamping before the addition changes edge behavior.

### World-coordinate overload `0x00565730`

- **Active in YR: Yes.** This sibling reads signed 32-bit world X/Y. Each cell coordinate is `(value + (value < 0 ? 255 : 0)) >> 8`, signed division by 256 truncating toward zero, not arithmetic-floor division.
- **Active in YR: Yes.** It applies the same signed fixed-stride linear test and null-slot check. On failure, it narrows the converted coordinates to two 16-bit words, stamps the same dummy `+0x24` dword, and returns `0x00ABDC50`.
- **Active in YR: Yes.** The sibling proves that `+0x24` is a packed coordinate writer shared by lookup families; it does not create a second fallback identity.

## 2. Dummy construction and Resize lifetime

- **Active in YR: Yes.** `CellClass::Constructor @ 0x0047BBF0` initializes the fixed object's relevant fields as follows. The widths shown are instruction widths, not inferred Rust choices.

| Field | Constructor/reset value | Width and interpretation | Transaction category |
|---|---:|---|---|
| `+0x24` | `(0,0)` | dword containing `x:i16,y:i16` | dummy process state; restamped on miss |
| `+0x38` | `0x0000FFFF` | dword tile identity/sentinel | dummy process constant in this fresh-load corridor |
| `+0x44` | `-1` | signed dword overlay identity | dummy process state; low Mark can mutate |
| `+0x48` | `-1` | dword sentinel, **not Land** | excluded from this handoff |
| `+0x4C` | `0` | dword reduced zone | reset fact only; dummy Recalc never changes it |
| `+0xEC` | `0` | dword Land enum (`Clear`) | reset fact only; dummy Recalc never changes it |
| `+0x116` | `-1` | word | outside bounded payload |
| `+0x11A` | `0` | byte sub-tile | dummy process state, unchanged here |
| `+0x11B` | `0` | signed byte level | existing dummy process state, unchanged by low Mark |
| `+0x11C` | `0` | byte slope/ramp | existing dummy process state, unchanged by low Mark |
| `+0x11D` | `0` | signed byte elevation/height | dummy process state, unchanged here |
| `+0x11E` | `0` | byte overlay data/state | dummy process state; low Mark can mutate |
| `+0x140` | modeled low flags cleared | dword flags; constructor preserves only its high native subset | existing dummy process state, unchanged by low Mark |

- **Active in YR: Yes.** The coordinate constructor source is zero during static initialization; startup instructions explicitly clear both words. Do not infer a nonzero initial coordinate from an untyped decompiler global.
- **Active in YR: Yes.** The two new-field reset values are instruction-pinned: `OR EAX,0xFFFFFFFF` at `0x0047BC1B`, dword store `EAX -> [ESI+0x44]` at `0x0047BC21`, and byte store zero-register `BL -> [ESI+0x11E]` at `0x0047BD1C`. Thus reset means signed identity `-1` and state `0`, not a Rust `Option` guess or pack byte `0xFF` copied into the cell.
- **Active in YR: Yes.** `MapClass::Resize @ 0x00565C10` unconditionally invokes the constructor in place on `0x00ABDC50` at `0x005670E7..0x005670F2`. The address/alias identity survives while constructor-owned fields reset.
- **Active in YR: Yes.** Neither a low-written dummy overlay identity nor state survives the next Resize: the in-place constructor overwrites both with `-1/0`. Persistence is from the write until another explicit writer or Resize reconstruction, not across reconstruction.
- **Active in YR: Yes.** Constructor masking at `+0x140` does not prove that the entire native dword is zero after every reconstruction. It proves the modeled low bridge bits are clear; Rust must not turn that into an unsupported claim about all high bits.
- **Active in YR: Yes.** The already-verified IsoMap miss corridor stamps the dummy coordinate but drops tile/subtile/level/slope/ice payload. Therefore a fresh authored OverlayPack pass reaches the dummy with tile sentinel `0xFFFF`, level `0`, and slope `0` unless another separately audited earlier owner changes it.

## 3. Exact low-Mark dummy/real writes

- **Active in YR: Conditional.** The procedural low branches are the four wood triggers `0x7A..0x7D` and four concrete triggers `0xE9..0xEC`. They are active YR code with retail types/art, conditional on authored trigger input. Their fixed and body write spans have the same field surface.

| Family/site | Overlay identity store | State store | Immediate call |
|---|---|---|---|
| wood fixed | `0x005FC976`, dword `cell+0x44` | `0x005FC97B`, byte `cell+0x11E` | `0x005FC981`, `RecalcAttributes(cell,-1)` |
| wood body | `0x005FCB67`, dword `cell+0x44` | `0x005FCB6A`, byte `cell+0x11E` | `0x005FCB70`, `RecalcAttributes(cell,-1)` |
| concrete fixed | `0x005FCDA4`, dword `cell+0x44` | `0x005FCDA9`, byte `cell+0x11E` | `0x005FCDAF`, `RecalcAttributes(cell,-1)` |
| concrete body | `0x005FCF95`, dword `cell+0x44` | `0x005FCF98`, byte `cell+0x11E` | `0x005FCF9E`, `RecalcAttributes(cell,-1)` |

- **Active in YR: Conditional.** Fixed/body ordinals write state bytes `0,1,2` in inner-loop order. The overlay identity store is a full dword even though active runtime IDs fit in a byte; `-1` remains the native none sentinel and must not be conflated with `0xFF` pack encoding.
- **Active in YR: Conditional.** Fixed-row clear probes read a dword at `cell+0x44` (`0x005FC916` wood, `0x005FCD44` concrete). A prior miss-side write can therefore make a later miss alias appear occupied. Dummy overlay persistence affects control flow, not only diagnostics.
- **Active in YR: Conditional.** Body writes do not reject missing or occupied results. They still draw, write the dword identity, write the state byte, and call Recalc. Multiple missing body cells repeatedly mutate the same dummy in exact traversal order.
- **Active in YR: Conditional.** Opposite-end search reads `+0x44` dword and requires `+0x11E == 1`, but its coordinate is first admitted by `Cell_in_bounds_check @ 0x00568300`. In the stock Size-diamond search corridor it observes allocated real cells rather than intentionally using the fallback.
- **Active in YR: Conditional.** Enumeration of all four `Get_CellClass -> stores -> Recalc` spans found no low fixed/body store to `+0x38`, `+0x4C`, `+0x48`, `+0xEC`, `+0x11A..+0x11D`, `+0x140`, or either compact cache.

### Post-Mark consumers before the next Resize

- **Active in YR: Conditional.** The first consumer is still inside authored loading: any later low trigger's unbounded fixed-row miss gets the same dummy and reads its dword `+0x44` at `0x005FC916/0x005FCD44`. A retained identity changes “clear row” into the occupied success-no-op arm.
- **Active in YR: Conditional.** Runtime repair keeps that identity live beyond Full_Init. `MapClass__RepairBridgeOrRestoreRamp_Low @ 0x00570050` scans a signed-word 5x5 neighborhood without a prior bounds predicate, calls `Get_CellClass @ 0x00570095`, and immediately reads dword `+0x44 @ 0x0057009A`; an invalid scan coordinate therefore reads the retained dummy identity. Wood low identity in `0x4A..0x65` dispatches `MapClass__RepairBridge_Low`. This is a direct pre-Resize reader, not only a theoretical stored value.
- **Active in YR: Conditional.** Runtime bridge-damage restamping supplies a direct state-byte reader when shared process flags admit the dummy. `MapClass__UpdateRamp_NS_DamageA_Low @ 0x0056ED40` falls back at `0x0056ED99..0x0056ED9F`, tests dummy/real `+0x140 & 0x80 @ 0x0056EDA4`, then reads `+0x11E @ 0x0056EDAD` and advances states `0..3 -> 4` or `5 -> 6`. An edge custom sequence can leave the already-modeled high/anchor flag on the shared dummy and then have low Mark overwrite its state; the later damage helper observes both. The sibling ramp-damage family has the same field shape.
- **Active in YR: Conditional.** `ProcessBridgeDamageStateMachine_Low @ 0x00571490` independently begins with the standard real-or-dummy lookup and later switches on `+0x11E` when its tile/bridge-flag admission succeeds. Together these readers prove that dummy state is exact process state until overwritten/Resize, even though dummy Recalc and whole-map sweeps never read it.

## 4. Dummy Recalc is a total no-op, not a rollback

- **Active in YR: Yes.** Cold instruction check at the entry of `CellClass::RecalcAttributes @ 0x0047D2B0` reproduced this control boundary: load receiver; compare it with absolute `0x00ABDC50` at `0x0047D2B8`; push the preserved register at `0x0047D2BE`; jump on equality at `0x0047D2BF` directly to common epilogue `0x0047DD5A`.
- **Active in YR: Yes.** The equality branch occurs before a Recalc helper call, coordinate-to-cache index, or relevant receiver read/write. Consequently the dummy call performs no Land derivation, RecalcZone, LAT, cache copy, level override, tile reset, overlay validation, state clear, neighbor mutation, animation work, or dirty work.
- **Active in YR: Conditional.** “Dummy Recalc no-op” does **not** mean “the complete low write is discarded.” Mark's explicit `+0x44/+0x11E` stores precede the call and persist; the dummy coordinate was already stamped by the lookup. Recalc neither commits nor rolls them back.
- **Active in YR: No.** Extending the dummy with mutable Land, reduced-zone, or compact-cache fields for this low-Mark transaction is unsupported. No such field is read or written on the dummy side of this Recalc guard.
- **Active in YR: Yes.** Dummy level, slope, bridge flags, and tile sentinel can matter to other audited consumers, but low Mark/Recalc does not mutate them. They remain distinct pre-existing process facts and are not evidence for widening the low transaction's new field set.

## 5. Real-cell Recalc field and consumer census

The table separates direct OverlayPack payload, real derived projection, dummy-only/process state, and excluded fields. “Payload” means the consumed-once real identity/state handoff to runtime OverlayGrid, not every live field in `ResolvedTerrainGrid`.

| Offset | Width/signedness | Real low/Recalc behavior | Downstream liveness | Required owner | Activity |
|---|---|---|---|---|---|
| `+0x24` | packed dword, two signed i16 | cell coordinate; low working coordinates select receiver | lookup/index/LAT neighborhood | real grid coordinate or dummy process state; not payload | **Active in YR: Yes** |
| `+0x38` | dword tile ID; `0xFFFF` sentinel | LAT/slope helper can replace current real tile or set sentinel | radar `0x0047BFA8/0x0047C001`; tile draw `0x00480389/0x004803D7`; bridge classifiers `0x0048675B/0x0048677B`; repair/damage walkers | derived terrain projection | **Active in YR: Yes** |
| `+0x44` | signed dword overlay ID; `-1` none | low Mark writes; Recalc reads and may clear to `-1` (`0x0047D378`, `0x0047D849`) | low probes/search; DrawOverlay; damage/repair; OverlayGrid runtime authority | finalized real payload; dummy process state | **Active in YR: Conditional** |
| `+0x48` | dword | constructor uses `-1`; this is not the Land field | no Land reader supports the stale claim | excluded | **Active in YR: No (as Land)** |
| `+0x4C` | dword reduced zone, values `0..7` | `RecalcZoneType @ 0x00483C80` writes it | copied to cache A byte `+0`; zone builders and movement hierarchy consume | derived terrain projection | **Active in YR: Yes** |
| `+0xEC` | dword Land enum | Recalc writes tile/overlay-derived Land; low retail identity can project Road | `RecalcZoneType 0x00483D2A`; passability `0x0048357D`; DrawOverlay; Drive/Ship/Infantry/Walk locomotion | derived terrain projection | **Active in YR: Yes** |
| `+0x11A` | byte sub-tile | may reset to zero on invalid/sentinel tile path | tile/render selection | derived terrain projection | **Active in YR: Yes** |
| `+0x11B` | signed byte level | read with `MOVSX`; written only when `levelOverride != -1`; low calls pass `-1` | LAT/neighbors; compact caches; zone flood height gate | existing terrain projection; not payload | **Active in YR: Yes** |
| `+0x11C` | byte slope/ramp | reset/recomputed for a real receiver; LAT reads current and neighbor slope | terrain movement/render/LAT | derived terrain projection | **Active in YR: Yes** |
| `+0x11D` | signed byte elevation/height | Recalc writes at `0x0047D993` | Tactical paths use signed reads at `0x006D809E/0x006D80B8` | derived terrain projection | **Active in YR: Yes** |
| `+0x11E` | byte overlay data/state | low Mark writes; OverlayData later overwrites; Recalc does not read it and only zeroes it when clearing identity (`0x0047D37F`, `0x0047D850`) | low opposite search; DrawOverlay; bridge damage/repair state machines | finalized real payload; dummy process state | **Active in YR: Conditional** |
| `+0x140` | dword flags | real Recalc can OR `0x20000` for attached tile animation; dummy guard excludes it | animation/bridge/terrain consumers outside payload | derived/process flags, not payload | **Active in YR: Yes** |

### Identity versus state during Recalc

- **Active in YR: Conditional.** Recalc reads full dword identity `+0x44`, resolves the overlay type, and can clear identity/state when the overlay is invalid for current terrain (including the active resource-on-slope path). A final payload captured before this validation can retain an identity native removed.
- **Active in YR: Yes.** There is no `+0x11E` read inside Recalc. The later OverlayData byte does not choose Land, zone, LAT, or cache output. “The final Recalc projects OverlayData” is therefore misleading: it preserves the overwritten state while identity and terrain/object facts drive projection.
- **Active in YR: Conditional.** The exact payload pair must be captured after OverlayData and the first real-cell whole-map Recalc, so any identity clear and accompanying state zero are represented. Derived fields remain beside it in resolved terrain rather than copied into the payload.

### Land and reduced zone

- **Active in YR: Yes.** `CellClass::RecalcZoneType @ 0x00483C80` first considers overlay identity and then reads Land from `cell+0xEC` at `0x00483D2A`; it writes `cell+0x4C` with full dword stores for reduced classes `0..7`. Object-list state can further affect the real result.
- **Active in YR: Yes.** `+0xEC`, not `+0x48`, is the active Land authority. The direct passability and locomotion readers make a wrong offset player-visible even if overlay rendering appears correct.
- **Active in YR: Conditional.** Retail `LOBRDG*`/`LOBRDB*` declarations prove that low procedural identity participates in active Land projection. Fixed/endpoint and many body sections declare `Land=Road`; endpoint rows use `NoUseTileLandType=true`. Some later body variants declare `NoUseTileLandType=false`, so Rust must resolve the exact selected type rather than hard-code every low body as Road.

## 6. Compact cache layout and reader closure

`RecalcAttributes` obtains a clamped zone index through `ZoneMap__CellToZoneIndex @ 0x0056D3F0` only after the dummy guard. The caches are real-cell derived state; the shared dummy never receives a cache slot from this call.

### Cache A: `Map+0x68` / global owner `0x0087F850`, stride 4

| Byte(s) | Meaning and writer | Active reader | Activity |
|---|---|---|---|
| `+0` | reduced-zone low byte copied from real `cell+0x4C` at `0x0047D571` | `ZoneMap__BuildZoneLevel @ 0x00581F90` skips class 7; incremental rebuild reads it | **Active in YR: Yes** |
| `+1` | raw level byte copied from `cell+0x11B` at `0x0047D560` | BuildZoneLevel copies it to cache B `+8` | **Active in YR: Yes** |
| `+2..+3` | `u16` base-zone ID; not written by per-cell Recalc, built by `MapClass__RebuildZoneConnectivity @ 0x0056C510` | `MapClass::GetZoneID @ 0x0056D230`; BuildZoneLevel and incremental rebuild | **Active in YR: Yes** |

### Cache B: `Map+0x70` / global owner `0x0087F858`, stride 10

| Byte(s) | Meaning and writer | Active reader | Activity |
|---|---|---|---|
| `+0..+1`, `+2..+3`, `+4..+5` | three `u16` hierarchical zone IDs, initialized/built outside per-cell Recalc | `Zone_precheck`, `AStar_main_loop`, `PathfinderClass__UpdateHierarchicalEdges` | **Active in YR: Yes** |
| `+6..+7` | `u16` base-zone ID copied from cache A `+2` by BuildZoneLevel/incremental rebuild | `ZoneMap__FloodFillScanline` base-zone match | **Active in YR: Yes** |
| `+8` | raw level byte; Recalc copies `cell+0x11B` at `0x0047D569`; BuildZoneLevel refreshes it from A `+1` | FloodFill requires neighboring absolute level difference `<2` | **Active in YR: Yes** |
| `+9` | unresolved byte/padding semantics | no implementation-relevant reader established in this slice | **Active in YR: No verified behavior** |

- **Active in YR: Yes.** The compact caches are not dead mirrors. `ZoneMap__BuildZoneLevel @ 0x00581F90`, `ZoneMap__FloodFillScanline @ 0x005824A0`, `MapClass__IncrementalRebuildZoneGraphAroundCell @ 0x00584550`, and the A*/hierarchy readers close their liveness.
- **Active in YR: Yes.** Per-real-cell Recalc writes only cache A reduced-zone/level bytes and cache B level byte in this compact corridor. Base and hierarchy IDs are rebuilt by their later owners. A finalized overlay payload must not pretend that the identity/state traversal itself owns those IDs.
- **Active in YR: No.** There is no dummy cache output. Adding cache bytes to `SharedCellDummy` would model a branch native never reaches.

## 7. LAT and invalid-neighbor semantics

- **Active in YR: Yes.** `CellClass__ApplyLAT_and_SlopeFixup @ 0x0047CA80` is called only for a real receiver because the dummy exits Recalc first. Within the bounded helper surface, its receiver tile write is the dword at `+0x38`.
- **Active in YR: Yes.** The helper reads current/neighbor tile `+0x38` and slope `+0x11C` (with level/coordinate logic). An invalid neighbor resolves through the normal shared dummy, whose fresh-corridor values are tile `0xFFFF` and slope `0`.
- **Active in YR: Yes.** Consequently the dummy tile sentinel has an observable edge-LAT role, but low Mark does not write it and dummy Recalc does not recompute it. Representing it as a constructor-stable constant is sufficient for this transaction unless a different audited earlier writer proves mutability.
- **Active in YR: Yes.** Real `+0x38` output is live in radar, terrain drawing, bridge tile classification, repair, and damage. It belongs to the resolved terrain projection and must remain synchronized with exact real-cell Recalc; it is not part of `FinalizedOverlayPayload`.

## 8. Load ordering and the correct finalized boundary

- **Active in YR: Conditional.** `ReadMapOverlayPacks @ 0x005FD2E0` constructs/Marks only accepted anchors after `Cell_in_bounds_check`; anchors are real. Low fixed/body walkers themselves can leave the playfield/allocated set and therefore can write the dummy.
- **Active in YR: Yes for `NewINIFormat>1`.** The OverlayData pass consumes every fixed-grid byte but performs its write only after the same real-cell admission. It writes `+0x11E` for allocated/in-bounds real cells, including identity-empty cells, and never writes the dummy.
- **Active in YR: Yes.** `ScenarioClass::Full_Init` calls the pack reader at `0x00687A34`, then a whole-real-map Recalc at `0x00687A5A`. That loop excludes the shared dummy.
- **Active in YR: Yes.** A later `MapClass::InitCellAttributes @ 0x00568BB0` call at `0x00687B92` contains another real-cell Recalc sweep at `0x00568DF4` after Terrain/authored objects. The first sweep is the transaction-3 pre-Terrain boundary; it is not the only Recalc later in load.
- **Active in YR: Conditional.** The first sweep validates/finalizes identity and projects Land/zone/LAT/cache from the post-Mark map while retaining the post-OverlayData state. The later object-phase sweep may change object-derived zone classification without making OverlayData a projection input.
- **Active in YR: Conditional.** Correct split:
  1. shared dummy retains process state `coord + overlay identity + state` (plus its independently owned existing level/slope/bridge bits and tile sentinel contract);
  2. real `ResolvedTerrainGrid` retains full derived projection, including Land/zone/LAT/cache and bridge facts;
  3. consumed-once `FinalizedOverlayPayload` carries one real-cell identity/state vector into runtime OverlayGrid;
  4. transaction 21 separately decides native restore authority and never replays Full_Init/Mark.

## 9. OpenTS navigation leads and disproved inheritance

- **Active in YR: No (authority).** OpenTS `code/map.cpp` helped locate the one global fallback `BlubCell` and its coordinate stamp; `code/cell.cpp` helped locate the early dummy Recalc return; `code/overlay.cpp` helped map fixed/body and OverlayData phases. None is evidence without the binary checks above.
- **Active in YR: Yes (binary-verified correspondence).** The useful correspondence is structural: one global dummy, in-place reconstruction, coordinate stamping, and an early Recalc return. `gamemd.exe` independently proves each.
- **Active in YR: No.** The inherited OpenTS comment that scratch-cell writes are “thrown away” is false for active YR behavior and even misleading for its persistent `BlubCell` implementation. Low `+0x44/+0x11E` writes remain until later callers overwrite/reconstruct them.
- **Active in YR: No (as exact layout).** OpenTS `CellZones`/`CellSubzones` names were navigation leads only. Active `gamemd.exe` fixes the exact 4-byte/10-byte layouts and writer/reader ownership above.

## 10. Current Rust ownership audit

- **Active in YR mismatch: Conditional.** `src/map/resolved_terrain.rs::SharedCellDummySnapshot` exposes only `coord`, `level`, `slope_type`, and `bridge_flags_0x1180`. `SharedCellDummy` packs those into one `Arc<AtomicU64>`; `fresh()`/`reconstruct_for_map_resize()` zero the word. There is no overlay dword sentinel, state byte, or tile-sentinel representation.
- **Active in YR preservation: Yes.** Existing tests correctly pin one identity and native-like coordinate stamping/reset: `gsi_04_01_resize_clears_bridge_bits_without_replacing_dummy_identity`, `gsi_04_01_isomap_misses_stamp_raw_coord_without_payload_leak`, `gsi_04_01_valid_isomap_lookup_does_not_stamp_dummy`, and `gsi_04_01_runtime_setter_uses_native_real_or_dummy_order`. Those mechanisms should be extended, not replaced.
- **Active in YR mismatch: Conditional.** `ResolvedTerrainGrid::build_inner` derives overlay Land/zone/passability from raw `map.overlays` before its late bridge-facts traversal. That traversal assigns anchor `BridgeCellFacts.overlay_id`, dispatches authored **high** stamps, then copies OverlayData to `BridgeCellFacts.state_byte`. It has no low procedural Mark, per-write real Recalc, dummy overlay/state mutation, or finalized payload.
- **Active in YR stale assumption: No.** `gsi_04_01_production_overlaypack_stamps_two_anchors_in_row_major_order` tests high-anchor stamping despite its broad name. There is no current `gsi_04_12_authored_fixed_map_mark...` test and no existing authored low-Mark proof to preserve under that assumed name.
- **Active in YR preservation: Yes.** `gsi_04_12_generated_materialized_overlays_never_replay_fixed_map_mark` is the correct generated-source exclusion and must remain green.
- **Active in YR mismatch: Conditional.** `src/sim/overlay_grid.rs::OverlayGrid::from_native_overlay_packs` decodes accepted entries again, repeats art/mode/slope filtering, invokes `recalc_overlay_passability` for each raw entry, then applies OverlayData. It cannot contain procedural fixed/body identities absent from the raw pack and creates a second overlay authority.
- **Active in YR mismatch: Conditional.** `recalc_overlay_passability` updates a useful overlay Land/reduced-zone/passability subset, but it is not the full per-write/native-final Recalc corridor: it does not own exact LAT tile/slope/elevation or compact-cache sequence and cannot repair the missing procedural traversal afterward.
- **Active in YR mismatch: Conditional.** Production app loading calls `OverlayGrid::from_native_overlay_packs` at `src/app/loading/init.rs:1117` and `:1987`. Both must eventually consume the one map-finalized payload rather than re-read pack/registry authority.

## 11. Implementation handoff

| # | Required delta | Exact owner/surface | Acceptance boundary | Risk |
|---:|---|---|---|---|
| 1 | Extend the **same** `SharedCellDummy` identity with signed-dword overlay identity (`-1` reset) and byte state (`0` reset), preserving coordinate stamp and Resize reconstruction. Keep Land/zone/cache out; expose tile `0xFFFF`/flat slope only as the edge-LAT contract needed by real Recalc. | `src/map/resolved_terrain.rs::{SharedCellDummy,SharedCellDummySnapshot}` and native real-or-dummy lookup helpers | Repeated invalid low writes observe prior identity/state; real hits do not stamp; Resize keeps identity but restores none/zero/sentinel defaults. | High: a fresh dummy or truncated byte identity changes fixed-row control flow. |
| 2 | In one authored-only traversal, execute exact sequential low Mark plus per-real-write Recalc, then OverlayData and the first real-cell sweep. Retain a map-native `FinalizedOverlayPayload` containing the resulting **real** identity/state pair; keep Land/zone/LAT/cache and bridge facts in `ResolvedTerrainGrid`. | narrow map low-Mark owner plus `src/map/resolved_terrain.rs`; exact raw Scenario adapter supplied by existing transaction design | Real Road/zone/LAT/cache match native ordering; dummy gets only explicit fields; generated-materialized bypass stays exact. | Critical: pre-deriving from raw packs leaves procedural identity and terrain projection divergent. |
| 3 | Move the finalized payload once into runtime overlay state and remove both production second-decode calls. Constructor accepts no OverlayPack/DataPack, registry, art set, game mode, Recalc, or RNG authority. Keep stream restore transaction 21 separate. | `src/sim/overlay_grid.rs::OverlayGrid::from_finalized_map_payload` (new narrow consumer) and `src/app/loading/init.rs` call sites `1117/1987` | OverlayGrid identity/state exactly equals the map-final vector, including procedural cells and Recalc-cleared cells; no second decode/filter/draw. | High: duplicate authority can silently restore raw trigger IDs over finalized bridge bodies. |

## 12. Exact proposed tests

1. `gsi_04_13_low_mark_dummy_persists_overlay_and_state_across_multiple_misses`
2. `gsi_04_13_negative_i16_alias_resolves_fixed_stride_before_dummy_fallback`
3. `gsi_04_13_dummy_recalc_preserves_explicit_overlay_and_state_writes`
4. `gsi_04_13_overlay_data_overwrites_real_cells_but_not_shared_dummy`
5. `gsi_04_13_finalized_payload_preserves_procedural_identity_without_second_decode`
6. `gsi_04_13_real_low_mark_recalc_projects_road_zone_lat_and_cache`
7. `gsi_04_13_dummy_resize_resets_overlay_none_state_zero_and_tile_sentinel`
8. `gsi_04_13_real_edge_lat_uses_dummy_tile_sentinel_and_flat_slope`

Each test is **Active in YR: Conditional** for low-trigger setup except negative-alias, Resize, and base lookup/lifecycle assertions, which are **Active in YR: Yes** independent of low-trigger content. Transaction-21 serialization tests are deliberately not proposed from this evidence.

## 13. Negative facts / do not implement

- **Active in YR: No.** Do not allocate a fresh fallback per miss, clear it after a caller returns, or give separate fixed/body/search paths different dummy identities.
- **Active in YR: No.** Do not apply per-axis `x/y` rectangle rejection before the signed `y*512+x` lookup; negative components can alias valid real slots.
- **Active in YR: No.** Do not make dummy Recalc derive Land, zone, cache, LAT tile, slope, or state. The native equality branch reaches the epilogue before those operations.
- **Active in YR: No.** Do not discard explicit dummy `+0x44/+0x11E` stores merely because the following Recalc is a no-op.
- **Active in YR: No.** Do not write OverlayData to the dummy; its real-cell admission excludes it.
- **Active in YR: No.** Do not use `+0x48` as Land. Active readers and writers use `+0xEC`.
- **Active in YR: No.** Do not put derived Land/zone/LAT/cache fields in the consumed overlay payload or make OverlayGrid reproject them from a second raw-pack decode.
- **Active in YR: No.** Do not claim OverlayData drives Recalc projection; Recalc has no state-byte read.
- **Active in YR: No.** Do not infer transaction-21 dummy persistence from fresh-load constructor/Resize behavior.

## 14. Stale-document wording to correct

- Replace “each missing lookup may use a harmless fresh default/scratch cell” with: **“all misses alias one persistent `CellClass` at `0x00ABDC50`; each miss stamps packed coordinate, and low Mark can leave overlay identity/state visible to later misses.”**
- Replace unqualified “dummy Recalc throws the write away” or “dummy Recalc no-op means edge writes vanish” with: **“Recalc itself exits before any effect, but Mark's preceding full-dword identity and byte-state stores persist.”**
- Replace the approved-design phrase “extend the dummy with overlay/state, Land/zone and cache fields actually read or written by low Mark/Recalc” with: **“extend it with overlay identity/state only for transaction 3; dummy Recalc never reads/writes Land, zone, LAT, or caches. Keep existing level/slope/bridge process fields and the constructor-stable tile sentinel separate.”**
- Replace [CELLCLASS_RECALCZONETYPE_00483C80_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_RECALCZONETYPE_00483C80_GHIDRA_REPORT.md) wording that places Land at `+0x48` with: **“Land is the dword at `CellClass+0xEC`; `+0x48` is a different constructor-sentinel field.”**
- Replace “the final Recalc projects the later OverlayData byte” with: **“OverlayData overwrites final state; Recalc does not read that byte, but validates identity and projects identity/terrain/object facts while preserving the state unless identity is cleared.”**
- Replace “a whole-map Recalc follows” when used as a complete load timeline with: **“the pre-Terrain first sweep follows OverlayData; `InitCellAttributes` later performs another real-cell sweep after Terrain/authored objects.”**
- Replace the OpenTS “scratch writes are thrown away” comment when cited as parity with the persistent-alias wording above.
- Replace any claim that an existing GSI-04.12 authored-low test already closes this with: **“the named current production row-major test covers high anchors; authored low Mark/finalized-payload tests are absent.”**

## 15. Open Questions Log and drain

| # | Question | Resolution | Activity/status |
|---:|---|---|---|
| 1 | Are packed x/y signed or unsigned? | Both are sign-extended i16 in `0x005657A0`. | **Active in YR: Yes — RESOLVED** |
| 2 | Is fallback decided by per-axis bounds? | No; signed `y*512+x`, capacity, and null slot decide. | **Active in YR: Yes — RESOLVED** |
| 3 | Can negative x/y alias a real cell? | Yes when signed linear result lands in a live slot. | **Active in YR: Yes — RESOLVED** |
| 4 | Does the world overload floor negative coordinates? | No; it truncates signed division toward zero. | **Active in YR: Yes — RESOLVED** |
| 5 | Are there multiple fallback objects? | No; both overloads return `0x00ABDC50`. | **Active in YR: Yes — RESOLVED** |
| 6 | What does a miss stamp? | Exact/narrowed packed coordinate dword at `+0x24` only. | **Active in YR: Yes — RESOLVED** |
| 7 | What are relevant constructor defaults? | Coord 0, tile `0xFFFF`, overlay -1, state 0, Land Clear, reduced zone 0, level/slope 0. | **Active in YR: Yes — RESOLVED** |
| 8 | Does Resize replace dummy identity? | No; constructor is called in place. | **Active in YR: Yes — RESOLVED** |
| 9 | Which fields do all low fixed/body sites write? | Dword `+0x44`, byte `+0x11E`, then Recalc(-1). | **Active in YR: Conditional — RESOLVED** |
| 10 | Does dummy Recalc execute a helper first? | No; equality branch precedes every relevant operation. | **Active in YR: Yes — RESOLVED** |
| 11 | Can dummy Recalc clear explicit state? | No; it reaches epilogue without a field write. | **Active in YR: Conditional — RESOLVED** |
| 12 | Does real Recalc read `+0x11E`? | No; only clear-to-zero sites accompany identity removal. | **Active in YR: Yes — RESOLVED** |
| 13 | Is Land `+0x48` or `+0xEC`? | `+0xEC`, dword; `+0x48` is not Land. | **Active in YR: Yes — RESOLVED** |
| 14 | What width is reduced zone? | Dword `+0x4C`, later narrowed into cache A byte 0. | **Active in YR: Yes — RESOLVED** |
| 15 | What is cache A's exact live layout? | 4 bytes: reduced zone, level, base-zone u16. | **Active in YR: Yes — RESOLVED** |
| 16 | What is cache B's exact live layout? | 10 bytes: three hierarchy u16s, base-zone u16, level byte, unresolved byte 9. | **Active in YR: Yes for bytes 0..8 — RESOLVED** |
| 17 | Are compact caches dead? | No; builders, flood fill, GetZoneID, A*, and hierarchy consume them. | **Active in YR: Yes — RESOLVED** |
| 18 | Can LAT run on a dummy receiver? | No through Recalc; dummy guard returns first. | **Active in YR: Yes — RESOLVED** |
| 19 | Can a real edge LAT query read the dummy? | Yes; neighbor miss supplies tile `0xFFFF`/slope 0. | **Active in YR: Yes — RESOLVED** |
| 20 | Does OverlayData overwrite the dummy? | No; admission permits only allocated/in-bounds real cells. | **Active in YR: Yes — RESOLVED** |
| 21 | Does either whole-map sweep include the dummy? | No; both iterate real allocated cells. | **Active in YR: Yes — RESOLVED** |
| 22 | What is the minimum finalized real payload? | Post-validation overlay identity and post-OverlayData state. | **Active in YR: Conditional — RESOLVED** |
| 23 | Must Land/zone/LAT/cache be duplicated in that payload? | No; they are live derived projection in resolved terrain. | **Active in YR: Yes — RESOLVED** |
| 24 | What is cache B byte `+9`? | Exact semantics not established; no relevant reader found and no transaction-3 decision depends on it. | **Active in YR: No verified behavior — DEFERRED, non-blocking** |
| 25 | Does native save/load serialize dummy overlay/state? | Outside fresh-load corridor; transaction 21 must verify independently. | **Active in YR: Unknown — DEFERRED to transaction 21** |
| 26 | Do shipped installed maps activate low triggers? | Prior bounded 385-payload census says no; not rerun here. Custom/editor maps remain compatible. | **Active in YR: Conditional — upstream result, non-blocking** |

Drain result: zero open question remains that can change transaction-3 dummy fields, real payload fields, Recalc split, cache/LAT ownership, or implementation/test handoff. The two deferred questions are outside the bounded decision surface and keep their owning later work open.

## 16. Adversarial questions

1. **Could the dummy branch land after a hidden Recalc side effect?** No. **Active in YR: Yes.** Cold disassembly places the absolute dummy compare before helpers/index/receiver field work and the `JZ` target is the common epilogue.
2. **Could low overlay identity be a byte store whose high bytes are irrelevant?** No. **Active in YR: Conditional.** All four sites use dword stores; probes/Recalc use dword reads and compare against signed `-1`.
3. **Could OverlayData consume every byte and therefore also stamp the dummy?** No. **Active in YR: Yes.** Byte consumption is separate from cell admission; the write occurs only for accepted real cells.
4. **Could all negative i16 coordinates safely fall back?** No. **Active in YR: Yes.** Fixed-stride signed linearization permits aliases such as `(-510,2) -> 514`.
5. **Could stale `+0x48` Land wording still be correct for a second Land copy?** No material evidence supports it. **Active in YR: Yes.** RecalcZone, passability, locomotion, and Recalc Land stores close on dword `+0xEC`.
6. **Could compact cache bytes be omitted as dead native redundancy?** No. **Active in YR: Yes.** BuildZoneLevel/FloodFill/GetZoneID/A*/hierarchy readers use them. They must be represented by derived terrain/zone owners, though not duplicated in overlay payload.
7. **Could the dummy state byte affect real Recalc projection through aliasing?** No. **Active in YR: Yes.** Dummy Recalc exits, real Recalc does not read `+0x11E`, and whole-map loops exclude the dummy.
8. **Could one global Recalc after OverlayData remove the need for per-write Recalc?** No. **Active in YR: Conditional.** Later Mark iterations/probes observe immediately projected real state and the dummy's explicit writes; exact sequential side effects and RNG/control flow precede the final sweep.

## 17. Cold spot-checks and zero-add pass

1. **Cold spot-check A — Active in YR: Yes.** Re-disassembly of `RecalcAttributes` entry confirmed compare against `0x00ABDC50` at `0x0047D2B8` and direct epilogue branch at `0x0047D2BF -> 0x0047DD5A`; no pre-guard helper or field access appeared.
2. **Cold spot-check B — Active in YR: Conditional.** Independent disassembly of both wood and concrete write families reproduced `Get_CellClass -> dword +0x44 -> byte +0x11E -> Recalc(-1)` in fixed and body paths, including the exact sites in section 3.
3. **Zero-additional-field pass — Active in YR: Conditional.** Enumerating all four low write spans added no field beyond `+0x44/+0x11E`; enumerating the dummy branch added no Recalc output; closing LAT/RecalcZone/cache writers/readers classified their fields as real derived projection or constructor-stable process facts. No new dummy or payload field was added after this pass.

## 18. Coverage ledger

| Corridor | Evidence closed | Result | Remaining owner |
|---|---|---|---|
| packed lookup and invalid signed coordinates | decompile + disassembly + alias example | exact signed fixed-stride resolution and one fallback | implementation |
| sibling world lookup | decompile + disassembly | trunc-toward-zero conversion; same fallback | implementation preservation |
| dummy constructor/Resize | constructor and call-site disassembly | identity persistent; exact relevant defaults | implementation |
| low fixed/body field surface | four independent instruction spans | only `+0x44` dword and `+0x11E` byte after coordinate stamp | implementation |
| dummy Recalc | cold entry disassembly | total no-op; explicit prior writes persist | implementation |
| real identity/state | Recalc reads/clear sites + Mark/Data ordering | post-validation identity + post-Data state payload | implementation |
| real Land/reduced zone | Recalc/RecalcZone writers + active readers | `+0xEC` dword Land, `+0x4C` dword reduced zone | resolved terrain projection |
| compact caches | writers + Build/FloodFill/GetZone/A* readers | exact 4/10-byte live layout through B+8 | zone/hierarchy projection |
| LAT edge | helper writer/readers + dummy defaults | real `+0x38` derived; dummy `0xFFFF`/slope0 neighbor input | resolved terrain + dummy constant |
| OverlayData/global sweeps | loader/Full_Init call sites | real-only Data and sweeps; two distinct load sweeps | implementation ordering |
| retail activation | rules/art + upstream corpus result | active compatible mechanism, shipped-corpus conditional | no blocker |
| current Rust owners/tests | direct source read | exact missing/duplicate owners identified | implementation |
| save/restore dummy serialization | excluded | not inferred | transaction 21 remains open |

## 19. Remaining uncertainty

- **Active in YR: Unknown, non-blocking here.** Cache B byte `+9` semantics remain unidentified. No audited writer/reader makes it relevant to the low transaction, payload, or current implementation decision.
- **Active in YR: Unknown, intentionally open.** Native transaction-21 serialization of dummy overlay/state was not investigated. Fresh-load state must not be generalized into restore authority.
- **Active in YR: Conditional.** The prior zero-trigger installed-map census was not rerun. This affects frequency wording, not executable behavior, retail declaration, or the required compatibility path.
- **Active in YR: Yes.** Decompiler local names/prototypes remain provisional. All load-bearing results use instruction widths/control flow and active readers rather than local names.
- No parity-blocking uncertainty remains for the bounded shared-dummy fields, Recalc dummy guard, real payload pair, Land/zone/LAT/cache ownership, OverlayData/global-sweep ordering, or current-Rust delta.

## 20. Ghidra annotation candidates (do not apply)

- `0x00ABDC50`: data-label candidate `g_MapClass_SharedFallbackCell`; plate: `persistent CellClass alias; misses stamp +0x24; Recalc exits; callers may persist other fields`.
- `MapClass::Get_CellClass @ 0x005657A0`: plate candidate `signed i16 y*512+x lookup; capacity/null gate; miss stamps shared dummy packed coord`.
- sibling `0x00565730`: function-name candidate `MapClass::Get_CellClass_At_WorldCoord`; plate candidate `signed /256 trunc toward zero; same fixed lookup and shared dummy`.
- `CellClass::RecalcAttributes @ 0x0047D2B0`: pre-comment candidate at `0x0047D2B8`: `fixed dummy equality guard; equal branch is total Recalc no-op to epilogue`.
- `CellClass::RecalcZoneType @ 0x00483C80`: plate candidate `reads Land dword +0xEC; writes reduced-zone dword +0x4C`.
- global owners `0x0087F850` and `0x0087F858`: candidates `g_ZoneCellCache4` and `g_HierarchicalZoneCellCache10`, with their exact stride layouts from section 6.
- `ZoneMap__BuildZoneLevel @ 0x00581F90`: plate candidate `A{reduced,level,baseZone} -> B{layerZone[3],baseZone,level}`.
- `ZoneMap__FloodFillScanline @ 0x005824A0`: plate candidate `requires B base-zone match and abs(level delta)<2; writes selected layer zone IDs`.

## Final status

**COMPLETE for `OVERLAYPACK_SHARED_DUMMY_FINAL_RECALC_FIELDS_REINVESTIGATION`.** The exact new dummy fields are overlay identity/state, the exact real finalized payload is post-validation identity/state, and Land/zone/LAT/cache remain live real derived projection. GSI-04.13 remains open until Rust implements and validates the transaction; transaction-21 dummy persistence remains separately open by design.
