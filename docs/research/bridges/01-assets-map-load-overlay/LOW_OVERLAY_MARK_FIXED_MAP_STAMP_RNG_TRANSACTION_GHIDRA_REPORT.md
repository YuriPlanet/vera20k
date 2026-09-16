# Low Overlay Mark: Fixed-Map Stamp and Scenario-RNG Transaction

Date: 2026-08-30
Status: **COMPLETE for the bounded inner-algorithm slice**
System rows: GSI-04.13 primary; GSI-04.12 source exclusion; GSI-04.15 negative Tube separation

Activity vocabulary in this report is literal. **Active in YR: Yes** means the behavior is reached by stock YR content. **Conditional** means active executable code plus retail-declared data/art can reach it, but the scanned shipped-map corpus does not contain its trigger. **No** marks a disproved/foreign mechanism. Every binary behavior below carries one of those labels; OpenTS is never parity evidence.

## Target question

Resolve the complete active-retail `OverlayClass::Mark @ 0x005FC570` transaction for authored low/water-bridge endpoint triggers: entry/activation predicates; both endpoint families and exact tables; signed coordinate and lookup order; three-cell fixed-end clear/write behavior; opposite-end search and termination; body length, traversal, overwrites, variants, exact RNG owner/calls/order; `RecalcAttributes`, LAT, zone, radar, and dirty effects; return/failure state; loader ordering; and evidence-backed exclusions.

## Non-goals

- Rust implementation, RMG generation internals, damage/repair, high-bridge stamping, or explicit/automatic Tube implementation.
- Declaring OpenTS behavior to be YR behavior. `C:\Users\enok\Documents\OpenTS\code\overlay.cpp:234..302` was used only to locate candidate tables and control flow; all facts were independently checked in active `gamemd.exe` and retail YR rules/data.
- Expanding the neighboring scenario-load report's Techno/RNG timeline except where it proves this transaction's owner and ordering.

## Evidence needed to mark COMPLETE / stop conditions

COMPLETE required: (1) decompile plus instruction evidence for every activation bound, signed coordinate operation, table, loop bound, lookup/write order, draw site, return arm, and side-effect call; (2) caller/xref proof for authored loading and RNG ownership; (3) INI/default plus binary reader proof for every load-bearing low-overlay property; (4) a read-only retail-map census; (5) direct current-Rust read; (6) five adversarial cases, two cold binary rechecks, and a zero-additional-mechanism pass. Any unresolved activation, loop-bound, RNG, Recalc, radar/dirty, or source term was a PARTIAL stop. None remains.

## Primary evidence ledger

- **Active in YR: Conditional.** Ghidra MCP read-only calls: `decompile_function(0x005FC570)`, `disassemble_function(0x005FC570)` (load-bearing ranges `0x005FC570..0x005FD227`, wood `0x005FC790..0x005FCB9F`, concrete `0x005FCBB9..0x005FCFCD`, common tail `0x005FD1FA..0x005FD227`), `get_function_callees(0x005FC570)`, and `read_memory(0x008333C0..0x00833447)`. No Ghidra metadata was mutated.
- **Active in YR: Conditional.** Supporting read-only decompile/disassembly: `ObjectClass::Mark 0x005F5850`, `ObjectClass::Unlimbo 0x005F4EC0`, `ObjectClass::MarkNeedsRedraw 0x005F4D10`, dirty owner `0x004F42F0`, `ObjectClass::UnInit 0x005F65F0`, `MapClass::Get_CellClass 0x005657A0`, bounds `0x00568300`, `CellClass::RecalcAttributes 0x0047D2B0`, LAT/slope `0x0047CA80`, zone `0x00483C80`, `Random::Next 0x0065C780`, and `ReadMapOverlayPacks 0x005FD2E0`.
- **Active in YR: Conditional.** Table construction is corroborated by disassembly stores into lazy statics at `0x00AC154C..0x00AC15B7`, guarded by bits `1/2/4/8` of `0x00AC1548`. `FUN_007C978A -> 0x007C970C` only registers static destructors with CRT `atexit`; it is not RNG or gameplay work.
- **Active in YR: Conditional.** Runtime IDs were mapped to names by dense registry order from retail-derived `C:\Users\enok\Documents\ra2-oracle-movement-emitter\ini\rulesmd.ini:[OverlayTypes]`, not by sparse numeric key. Properties were checked in sections `LOBRDG01..25`, `LOBRDGE1..4`, `LOBRDB01..25`, and `LOBRDGB1..4`; binary readers/defaults are `OverlayTypeClass::ReadINI 0x005FE770` and constructor `0x005FE250`.
- **Active in YR: Conditional.** The companion activation report proves `Full_Init -> ReadMapOverlayPacks @ 0x00687A34`, y-major/x-minor construction, constructor -> Unlimbo -> virtual `Mark(1)`, and the later OverlayData pass. This report independently rechecked the relevant bodies and call sites.

## Activation and lifetime

- **Active in YR: Conditional.** Authored packed overlays reach the function only when `[Basic] NewINIFormat>1`, the decoded ID is not `0xFF`, the cell is accepted/in-bounds and allocated, the type exists, and it has SHP art or `CellAnim`; the reader constructs one ephemeral `OverlayClass` synchronously in decoded order. Retail endpoint sections name loadable images. (`ReadMapOverlayPacks 0x005FD2E0`, constructor call `0x005FD4D2`, `Unlimbo` dispatch `0x005F4FB0..0x005F4FB4`.)
- **Active in YR: Conditional.** Inside `Mark`, `ObjectClass::Mark(this,mark)` must first return true; derived processing then requires `mark==1 || mark==3`. The packed-loader call is `mark=1`. (`0x005FC58A..0x005FC5A4`.)
- **Active in YR: Conditional.** The coordinate is obtained through vtable `+0x1B8`; the ordinary object coordinate converts world X/Y to cells with signed, toward-zero division by 256 and is packed as two signed i16 components. (`ObjectClass::Get_Cell_Packed 0x0041BEA0`; entry call `0x005FC5AA..0x005FC5C6`.) All endpoint additions are 16-bit stores/adds; lookups and distance arithmetic sign-extend those i16 values.
- **Active in YR: Conditional.** Universal derived gate: if initiating `Cell+0x11C SlopeIndex > 4`, return false unless the unrelated overlay runtime ID is exactly `0xB2`. Both low trigger ranges therefore reject slopes 5..255 before any low fixed/body write. (`0x005FC5E0..0x005FC5F4`.) Loading suppression bypasses later ordinary placement checks, but not this gate.
- **Active in YR: Conditional.** The only procedural low trigger ranges are inclusive `0x7A..0x7D = LOBRDGE1..4` and `0xE9..0xEC = LOBRDGB1..4`; comparisons are `0x005FC790..0x005FC7A2` and `0x005FCBB9..0x005FCBCB`. Body/fixed IDs outside those ranges take ordinary Mark and never recurse into this transaction.
- **Active in YR: Conditional.** Once either family branch is selected, it bypasses ordinary passability/`Overrides` placement checks. After any branch-local no-op or writes, it always uses the common success tail, returns true, and logically deletes the ephemeral overlay object.
- **Active in YR: No (stock shipped-map activation).** A read-only LCW OverlayPack census found zero trigger bytes in 385 shipped/installed payloads: 53 loose maps; `mapsmd03.mix` 14; `multimd.mix` 173; `MAPS01.MIX` 17; `MAPS02.MIX` 17; `MULTI.MIX` 97; `expandmd01.mix` 13; plus `RandMap.Sed` 1. The branch is therefore content-conditional for authored/editor/custom YR maps, not stock-map-active and not dormant/TS-only.

## Exact family tables

**Active in YR: Conditional for every row.** Direction enum was independently recovered from startup writes in `InitializeDirectionOffsets 0x0049F2F0..0x0049F39B`: `0=N(0,-1)`, `2=E(1,0)`, `4=S(0,1)`, `6=W(-1,0)`.

| Trigger / index | New fixed row, states 0/1/2 | Search direction | Exact required opposite center (`state==1`) | Body orientation/base |
|---|---|---|---|---|
| `LOBRDGE1 0x7A` / 0 | N,origin,S = `LOBRDG19 0x5C` | W | `LOBRDG21 0x5E` | EW, `LOBRDG01 0x4A` |
| `LOBRDGE2 0x7B` / 1 | N,origin,S = `LOBRDG21 0x5E` | E | `LOBRDG19 0x5C` | EW, `LOBRDG01 0x4A` |
| `LOBRDGE3 0x7C` / 2 | W,origin,E = `LOBRDG23 0x60` | S | `LOBRDG25 0x62` | NS, `LOBRDG10 0x53` |
| `LOBRDGE4 0x7D` / 3 | W,origin,E = `LOBRDG25 0x62` | N | `LOBRDG23 0x60` | NS, `LOBRDG10 0x53` |
| `LOBRDGB1 0xE9` / 0 | N,origin,S = `LOBRDB19 0xDF` | W | `LOBRDB21 0xE1` | EW, `LOBRDB01 0xCD` |
| `LOBRDGB2 0xEA` / 1 | N,origin,S = `LOBRDB21 0xE1` | E | `LOBRDB19 0xDF` | EW, `LOBRDB01 0xCD` |
| `LOBRDGB3 0xEB` / 2 | W,origin,E = `LOBRDB23 0xE3` | S | `LOBRDB25 0xE5` | NS, `LOBRDB10 0xD6` |
| `LOBRDGB4 0xEC` / 3 | W,origin,E = `LOBRDB25 0xE5` | N | `LOBRDB23 0xE3` | NS, `LOBRDB10 0xD6` |

**Active in YR: Conditional.** Raw table evidence: fixed row directions `[4,4,2,2]` at `0x008333C0/0x00833408`; fixed IDs at `0x008333D0/0x00833418`; join directions `[6,2,4,0]` at `0x008333E0/0x00833428`; body bases at `0x008333F0/0x00833438`; opposite tables indexed by `join/2`, wood `[0x60,0x5C,0x62,0x5E]` at `0x008333F8`, concrete `[0xE3,0xDF,0xE5,0xE1]` at `0x00833440`. Lazy start offsets are `[(0,-1),(0,-1),(-1,0),(-1,0)]`; body cross-offset rows are `[(-1,0),(0,0),(1,0)]` and `[(0,-1),(0,0),(0,1)]` at `0x00AC1588/0x00AC15A0` after initialization.

## Fixed-end transaction

- **Active in YR: Conditional.** The clear probe begins at `origin + start_offset`, performs exactly three `MapClass::Get_CellClass` calls in fixed-row direction, and checks `Cell+0x44 == -1`. It does not short-circuit after a conflict; all three lookups occur in N/origin/S or W/origin/E order. (`0x005FC8C0..0x005FC930`, concrete `0x005FCCE9..0x005FCD54`.)
- **Active in YR: Conditional.** If any probe is occupied, the transaction makes no fixed/body write and no RNG call, skips search, then enters common success cleanup. The fake trigger is not persisted. The original cell is nevertheless Recalc'd once at the tail using its pre-existing overlay.
- **Active in YR: Conditional.** If clear, it restarts from the same first coordinate and makes exactly three writes in the same order. Each write stores the row's one fixed overlay ID at `Cell+0x44`, writes ordinal `j=0,1,2` to `Cell+0x11E`, then immediately calls `RecalcAttributes(cell,-1)` before advancing. (`0x005FC930..0x005FC9AD`, concrete `0x005FCD54..0x005FCDD1`.)
- **Active in YR: Conditional.** The triggering origin is ordinal 1, so its fake endpoint ID is overwritten by the fixed center ID. Fixed-end writes consume **zero** RNG calls—raw, ranged, fixed-range, or otherwise.
- **Active in YR: Conditional.** `Get_CellClass 0x005657A0` computes `index=(signed y*512)+signed x`, accepts only `0..0x3FFFF` with a non-null sparse pointer, otherwise returns the one dummy at `0x00ABDC50` after stamping only dummy coordinate `+0x24`. Clear/write rows do not bounds-check, so multiple missing coordinates alias one persistent dummy. Dummy overlay/state writes are observable to later probes; its `RecalcAttributes` immediately returns. Current normal retail map geometry generally keeps the trigger row allocated, but exact alias/order is native behavior, not license to drop edge writes.

## Opposite-end search and termination

- **Active in YR: Conditional.** Search begins at `origin + join_direction`, not at the fixed-row start. Before the first lookup and after every advance it calls `Cell_in_bounds_check 0x00568300`. This signed i16 diamond predicate is: `W < x+y`, `x-y < W`, `y-x < W`, and `x+y <= W+2H`, using `Map+0xF4 W` and `Map+0xF8 H`.
- **Active in YR: Conditional.** At each in-bounds coordinate, the loop gets the cell and succeeds only when both `Cell+0x44 == exact family/direction opposite ID` and byte `Cell+0x11E == 1`. Wrong state, other fixed ends, other family, body overlays, and arbitrary blockers are ignored and scanned through. First exact match wins. (`0x005FC9D1..0x005FCA82`, concrete `0x005FCDF5..0x005FCEB0`.)
- **Active in YR: Conditional.** There is no bridge-length limit, blocker termination, wrap, or second direction. The sole failure termination is leaving the playfield predicate. A miss leaves the newly written three-cell fixed end in place, consumes no RNG, and returns true after common cleanup.

## Body geometry, order, and overwrites

- **Active in YR: Conditional.** On match, compute `reverse=(join-4)&7` (the direction from found end toward the trigger), then step once from the found center. This is the first body-row center. (`0x005FCA82..0x005FCAA5`, concrete `0x005FCEB0..0x005FCED3`.)
- **Active in YR: Conditional.** Length is **not** measured to the trigger center. It is the Chebyshev distance from that first body center to the first transverse cell of the newly written fixed row: `L=max(abs(signext_i16(work.x)-signext_i16(start.x)), abs(signext_i16(work.y)-signext_i16(start.y)))`. x86 `CDQ/XOR/SUB` implements signed absolute value. (`0x005FCAA5..0x005FCAE4`, concrete `0x005FCED3..0x005FCF12`.)
- **Active in YR: Conditional.** For valid cardinal tables and center separation `D>=1`, this simplifies to `L=max(D-1,1)`. The defensive `if L>0` branch is always taken after a real match under active map bounds. For normal `D>=2`, it writes all centerline rows strictly between endpoints, starting beside the previously found end and moving toward the new trigger.
- **Active in YR: Conditional.** Exact adversarial edge: adjacent centers (`D=1`) still yield `L=1` because the transverse start-cell difference is one. The one body row is centered on the newly written trigger and overwrites all three just-written fixed cells with body variants/states 0/1/2. This is not an approximation or an OpenTS-only quirk; the disassembly operands use the retained start-row x/y.
- **Active in YR: Conditional.** Body orientation is `((reverse & 3)/2)`: reverse N/S selects base `LOBRDG10/LOBRDB10` and j offsets W,center,E; reverse E/W selects base `LOBRDG01/LOBRDB01` and j offsets N,center,S. Outer order is opposite-end side toward trigger; inner order is exactly j=0,1,2 in those offset orders.
- **Active in YR: Conditional.** Body cells are neither bounds-checked nor tested for occupancy. Each `Get_CellClass` result is overwritten even if it already contains an overlay. Missing rows alias the shared dummy exactly as above. After every three writes, work advances one reverse-direction cell; outer loop executes exactly `L` times.

## Variant selection and exact RNG transaction

- **Active in YR: Conditional.** Each body cell performs this exact order: lookup cell; call `Random::Next`; store `body_base + (raw & 3)` at `+0x44`; store j at `+0x11E`; call `RecalcAttributes(cell,-1)`. Wood assembly is `0x005FCB44..0x005FCB70`; concrete is `0x005FCF72..0x005FCF9E`.
- **Active in YR: Conditional.** The receiver is exactly `[g_ScenarioClass_Instance @ 0x00A8B230] + 0x218`, loaded immediately before `Random::Next @ 0x0065C780`. It is neither Main RNG nor MapGen RNG. A successful body consumes exactly `3*L` **raw Scenario calls** in outer/inner order.
- **Active in YR: Conditional.** There is no `RandomRanged` helper, rejection, modulo, or `next_range(0,3)` call. There are also no fixed-range draws: fixed-end rows, static-table initialization, clear probes, search probes, occupied-row success-no-op, missing opposite, and common cleanup each consume zero RNG.
- **Active in YR: Conditional.** `Random::Next` checks receiver byte `+0`: if nonzero, it returns zero and does not advance. Otherwise it XOR-updates one of 250 state dwords at `+0xC` using dword indices `+4/+8`, returns the updated raw u32, increments both indices, and wraps each after 249. Thus disabled RNG still receives exactly `3*L` calls and selects each base variant, but its cursor is unchanged. (`decompile_function/disassemble_function 0x0065C780..0x0065C7D0`.)
- **Active in YR: Conditional.** OverlayPack construction is y=0..511 outer, x=0..511 inner; each endpoint transaction completes before the next packed coordinate. The endpoint encountered later in that row-major order normally finds the earlier fixed center and owns the `3*L` draw sequence. Later packed overlay rows may overwrite procedural writes without undoing draws.

## Recalc, LAT, zone, radar, dirty, and cleanup

- **Active in YR: Yes when any declared low overlay is Recalc'd; Conditional for trigger-generated calls.** Every successful fixed/body write calls `CellClass::RecalcAttributes(cell,-1)` immediately; `-1` preserves `Cell+0x11B Level`. The common tail calls it once more on the original initiating cell, so a clear trigger center is Recalc'd as fixed ordinal 1 and then again at cleanup. Dummy Recalc returns immediately. (`0x0047D2B0`, Mark call sites above and `0x005FD1FA`.)
- **Active in YR: Yes.** Retail endpoint/fixed/body types declare `Land=Road`, `NoUseTileLandType=true`, `RadarColor=92,92,92`; constructor defaults are Land Clear, Tiberium=false, and NoUse=true, and `ReadINI @ 0x005FE798..0x005FE7B7/0x005FE958..0x005FE973` supplies the retail values. Dense runtime IDs, not sparse INI keys, select metadata.
- **Active in YR: Yes.** For these types Recalc stores overlay Land at `Cell+0xEC`, refreshes slope from the pristine TMP when available, cannot take the Tiberium-on-slope clear because retail low types have Tiberium=false, applies the active `CliffBackImpassability` neighbor test (stock mode 2 may reclassify qualifying land to 3), calls `ApplyLAT_and_SlopeFixup 0x0047CA80`, calls `RecalcZoneType 0x00483C80`, then updates compact zone/level buffers. LAT/slope fixup reads neighbors and can replace only this cell's tile ID `+0x38`; zone writes `Cell+0x4C` and the compact buffers.
- **Active in YR: No.** The later automatic `TubeClass` construction branch in Recalc requires `LandType==10`. Low overlay `NoUseTileLandType=true` takes the earlier overlay-owned return path after LAT/zone, so no automatic Tube is created. Explicit `[Tubes]` are loaded by a separate earlier owner. GSI-04.15 must not model fixed low decks as tube-backed.
- **Active in YR: No (direct per-cell invalidation).** Neither the low branch nor Recalc/LAT/zone calls a radar-pixel/cache invalidator. `RadarColor` is metadata consumed by normal radar rendering; it does not cause an immediate per-written-cell dirty action. The Mark callee list contains no radar routine.
- **Active in YR: Yes once per accepted ephemeral trigger object.** Before the derived branch, successful `ObjectClass::Mark` sets `IsOnMap +0x74=1` and invokes `MarkNeedsRedraw 0x005F4D10`; with the constructor-cleared flag it sets `NeedsRedraw +0x80=1` and calls `0x004F42F0(0)`, which sets `g_Tactical+0xD7D=1`. Argument zero means no bridge counter increment. There is no additional tactical dirty call per fixed/body cell.
- **Active in YR: Conditional.** Common tail `0x005FD1FA..0x005FD227`: Recalc original cell; clear object `IsOnMap +0x74=0`; set `InLimbo +0x81=1`; dispatch primary vtable `+0xF8 = ObjectClass::UnInit 0x005F65F0`; return 1. Constructor/vtable proof: constructor writes primary vtable `0x007EF3D4`; `+0x124` is Mark and `+0xF8` is UnInit; RTTI descriptor at `0x00833458` is `.?AVOverlayClass@@`.
- **Active in YR: Conditional.** Entry failures (base Mark false, mark not 1/3, or slope gate) return 0 without low writes, low RNG, tail Recalc, or tail deletion. `Unlimbo` then restores only `InLimbo +0x81=1`; the coordinate write, base Mark's `IsOnMap/NeedsRedraw`, and tactical dirty side effects are not transactionally rolled back. Branch-local failures after family selection are success-no-ops/partial success, not false.
- **Active in YR: Yes for authored format>1 loads.** After all Mark calls, `ReadMapOverlayPacks` makes a second complete y/x pass and blindly writes decoded OverlayData bytes to allocated cells' `+0x11E` (`0x005FD5F7..0x005FD656`) without RNG or immediate Recalc; Full_Init's later global Recalc observes those final state bytes. OverlayData therefore overrides procedural 0/1/2 states.

## Adversarial closure cases

1. **Active in YR: Conditional.** One of three fixed-row cells occupied: all three probes still occur; zero writes/search/draws; original-cell tail Recalc; return true.
2. **Active in YR: Conditional.** Clear row, no opposite before bounds: exactly three fixed writes/Recalcs persist; zero draws; original center gets a second Recalc; return true.
3. **Active in YR: Conditional.** Wrong opposite ID or correct ID with state 0/2: scan passes through it; only exact ID+state1 terminates; first exact candidate wins.
4. **Active in YR: Conditional.** Adjacent exact endpoints: `L=1`; three raw Scenario calls overwrite the newly fixed trigger row with body variants; tail Recalc follows.
5. **Active in YR: Conditional.** Body crosses occupied/missing cells: existing overlays are overwritten; missing lookups mutate the one dummy in exact j/row order; dummy Recalc is a no-op; draws are still consumed.

## Current Rust implication (direct read, not implementation)

- **Active in YR mismatch: Conditional.** `src/map/overlay.rs:138..176` decodes OverlayPack row-major and captures OverlayData, but `src/map/resolved_terrain.rs:2438..2481` only dispatches authored high-anchor stamps and then applies OverlayData. No `0x7A..0x7D/0xE9..0xEC` transaction exists.
- **Active in YR mismatch: Conditional.** `ResolvedTerrainGrid` currently derives overlay terrain/passability before the late bridge-facts overlay loop. Exact low Mark must mutate the owning cell overlay identity/state and apply the same Recalc/LAT/zone consequences in write order, not merely synthesize structural bridge facts afterward.
- **Active in YR mismatch: Conditional.** `ScenarioFillRng` at `src/sim/scenario_bootstrap.rs:1722..1729` exposes only inclusive ranged draws, and headless terrain wiring at `src/headless_scenario.rs:90..121` passes only that ranged callback. Low Mark needs a narrowly owned raw Scenario call seam; routing through Main, MapGen, or the ranged helper is wrong.
- **Active in YR mismatch: Conditional.** `SharedCellDummySnapshot` / `SharedCellDummy` at `src/map/resolved_terrain.rs:421..429,774..883` models coord, level, slope, and selected bridge flags, but not overlay ID/state/Land/zone. Exact low edge semantics require extending the same persistent identity or proving the affected coordinates unreachable at the accepted source boundary.
- **Active in YR preservation requirement: Yes.** Keep `OverlayLoadSource::GeneratedMaterialized` and `gsi_04_12_generated_materialized_overlays_never_replay_fixed_map_mark`: accepted `.SED` generation directly stamps completed low decks and must consume zero fixed-Mark Scenario draws.

## Implementation handoff

| # | Behavior -> Rust delta -> surface | Acceptance -> proposed test | Risk |
|---:|---|---|---|
| 1 | **Conditional:** authored format>1 packed endpoints execute synchronously in decoded order; generated-materialized never does -> add one authored provenance/format-gated low-Mark owner inside the overlay traversal -> map source/load gate + `resolved_terrain` | Earlier endpoint leaves fixed row; later endpoint finds it and materializes both families in y/x order -> `gsi_04_13_authored_low_endpoint_pairs_expand_in_overlaypack_order` | High: wrong phase changes topology and all later RNG. |
| 2 | **Conditional:** exact signed i16 rows/search/first-match/`L=max(D-1,1)` plus dummy alias and partial-success arms -> encode recovered tables and native lookup semantics, preserving three clear probes, writes, overwrites, and common-tail result -> narrow low-stamp module + shared dummy | occupied row, no opposite, wrong state, first-of-two candidates, adjacent endpoints, and edge dummy fixtures match exact IDs/states/returns -> `gsi_04_13_low_endpoint_search_failure_and_adjacent_overwrite_edges` | High: ordinary custom maps diverge; edge aliases can alter later probes. |
| 3 | **Conditional:** body uses `3*L` raw Scenario words, raw&3, opposite-to-trigger/j order; every other arm zero -> expose a raw draw only to the authored low-Mark owner and borrow it before later Scenario consumers -> `ScenarioFillRng`, headless/app terrain bootstrap | Seeded family A/B variants and post-load cursor match a reference raw stream; fixed/no-op cases preserve cursor -> `gsi_04_13_low_endpoint_rng_is_three_raw_scenario_draws_per_body_row` | Critical: a ranged draw shifts deterministic later consumers. |
| 4 | **Yes/Conditional:** every write immediately recalculates; tail repeats origin; data pack later wins; no per-cell radar dirty or Tube -> route writes through the existing exact overlay land/LAT/CliffBack/zone projection, retain one object-level tactical dirty semantic only if that state is modeled -> overlay registry/resolved terrain/data phase | Road/zone/tile consequences match after conflicting OverlayData; zero automatic tubes and zero per-cell dirty counts -> `gsi_04_13_overlaydatapack_overwrites_mark_state_without_recalc_then_final_recalc` | High: IDs can look right while passability/LAT/zone is stale. |
| 5 | **Yes preservation:** body IDs and generated `.SED` decks never enter trigger dispatch -> restrict dispatch to the two four-ID ranges and authored provenance -> overlay type dispatch/source tests | Lostlake/Killer/Shrapnel body rows remain unchanged; generated materialized test remains green -> `retail_low_body_ids_never_expand_or_consume_scenario_rng` plus existing GSI-04.12 test | High-frequency stock regression if ordinary bodies are misclassified. |

## Negative facts / do not do

- **Active in YR: No.** Do not port OpenTS enums, TS map bounds, TS RNG ownership, or TS Tube interpretation; it was a navigation lead only.
- **Active in YR: No.** Do not dispatch body ranges `0x4A..0x65/0xCD..0xE8`, high anchors `0x18/0x19/0xED/0xEE`, generated direct decks, damage/repair overlays, or Tube rows through this endpoint mechanism.
- **Active in YR: No.** Do not use a post-load connected-component pass, nearest-end search, blocker stop, length cap, second-direction retry, center-to-center length, per-row variant, deterministic coordinate hash, fixed-range draw, or inclusive-range RNG call.
- **Active in YR: No.** Do not roll back the fixed end when no opposite exists, roll back RNG after overwrites, short-circuit the three clear probes, skip occupied body cells, or clamp body writes to the Rust rectangle without reproducing shared-dummy semantics.
- **Active in YR: No.** Do not infer immediate radar invalidation from `RadarColor`, per-cell tactical dirties from Recalc, bridge-counter increments from the base dirty call, or automatic Tube creation from the word “bridge.”

## Remaining uncertainty

- **No parity-blocking uncertainty remains** for activation, family/range mapping, signed coordinates, lookup/write/search/body order, opposite termination, body bound, RNG owner/call kind/count/order, Recalc/LAT/CliffBack/zone effects, radar/dirty effects, return/failure state, OverlayData precedence, or source exclusions.
- **Non-load-bearing:** decompiler local variable names and some recovered C++ prototypes remain provisional; all handoff-critical claims above are tied to instruction ranges, memory tables, callers, defaults/readers, and retail data rather than those names.
- **Corpus boundary:** zero triggers is a complete result for the 385 installed/shipped payloads scanned, not a claim that arbitrary retail-compatible custom/editor maps cannot activate the code. Hence the mechanism remains **Conditional**, not dormant.

## Stale-document wording to correct

- Replace any “low bridges are Tube-backed / Recalc constructs a Tube for low deck overlays” wording (including inherited wording in [BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md) or [BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md)) with: “Retail low endpoint/fixed/body overlays are `Land=Road, NoUseTileLandType=true`; Recalc takes its early overlay LAT/zone return and cannot reach automatic `LandType==10` Tube construction.”
- Replace “fixed low endpoint Mark is TS-only/dormant” with: “active YR executable and retail-declared authored-content mechanism, conditional because zero trigger cells occur in the 385 shipped payloads scanned.”
- Replace any “fixed IDs are the sparse `[OverlayTypes]` key minus one” wording with dense runtime insertion mapping; e.g. runtime `0x5C` is `LOBRDG19`, and runtime `0x7A` is `LOBRDGE1`.
- Replace any “length is endpoint-center distance” or “body starts at the new trigger” wording with the retained-start-cell Chebyshev formula and opposite-to-trigger traversal, including the adjacent-end one-row overwrite.
- Replace any “low variants use ranged Scenario draw 0..3” wording with “one raw `Random::Next` per body cell, variant `raw&3`; no fixed/ranged draw.”

## Ghidra annotation candidates (do not apply)

- `OverlayClass::Mark @ 0x005FC570`: prototype candidate `bool __thiscall OverlayClass::Mark(int mark)`; plate candidate `low triggers 7A..7D/E9..EC: fixed row, first exact opposite, raw Scenario body transaction`.
- `0x008333C0/0x00833408`: `low_fixed_row_direction_by_trigger`; `0x008333D0/0x00833418`: `low_fixed_overlay_by_trigger`; `0x008333E0/0x00833428`: `low_join_direction_by_trigger`.
- `0x008333F0/0x00833438`: `low_body_variant_base_by_orientation`; `0x008333F8/0x00833440`: `low_opposite_fixed_center_by_join_halfdir`.
- `0x00AC154C/0x00AC155C`: `low_fixed_start_offsets_lazy`; `0x00AC1588/0x00AC15A0`: `low_body_cross_offsets_lazy`; `0x00AC1548`: `low_mark_static_table_init_guard_bits`.
- `FUN_004F42F0 @ 0x004F42F0`: candidate `MapClass::SetTacticalDirtyAndOptionalBridgeState`; its Mark caller passes zero.

## Closure checks

1. Cold re-decompile of `Mark 0x005FC570` and `Random::Next 0x0065C780` reproduced the tables, exact `raw&3` draw sites, start-cell length operands, and zero ranged/fixed-range calls.
2. Cold raw-memory read of `0x008333B0..0x0083345F` reproduced both families' fixed/join/base/opposite constants; dense retail `[OverlayTypes]` enumeration reproduced every runtime name.
3. Zero-additional-mechanism pass over `get_function_callees(0x005FC570)` found no radar, Tube, ranged RNG, per-cell dirty, alternative search, or rollback call. The complete relevant callee set is `ObjectClass::Mark`, `MapClass::Get_CellClass`, bounds, `Random::Next`, `RecalcAttributes`, and lifecycle support; unrelated overlay branches were excluded by ID control flow.
