# Low Overlay Mark: Scenario-Load Activation Boundary

Date: 2026-08-30
Status: **COMPLETE for the bounded activation/order/source-discriminator slice**
System: active YR low/water bridge map loading (GSI-04.13; GSI-04.12 interaction only where the shared high-stamp pass establishes order)

`[ACTIVE-YR]` means decompiled active-retail `gamemd.exe` control flow plus disassembly/caller evidence, not an OpenTS inference. Unless a paragraph is explicitly labelled `NEGATIVE`, `RUST`, `RETAIL-DATA`, or `UNCERTAINTY`, every binary behavior claim below is `[ACTIVE-YR]`.

## Target question

Prove the complete scenario-load activation and ordering boundary around `OverlayClass::Mark @ 0x005FC570`: which authored low endpoint/body rows reach it; where high stamping, `[OverlayDataPack]`, final terrain recalculation, authored Techno construction, and their RNG draws occur; why accepted `.SED` generation instead keeps the RMG's already-materialized deck and never replays low Mark; and what exact source discriminator Rust must preserve.

## Non-goals

- The endpoint tables' exact coordinate geometry, scan direction, opposing-end identity, and variant-table contents beyond call-boundary/draw-count facts.
- RMG bridge eligibility/geometry, high-bridge stamp implementation, bridge damage/repair, TubeClass behavior after parsing, or Rust edits.
- TS/OpenTS behavior as parity authority. `C:\Users\enok\Documents\OpenTS\code\overlay.cpp`, `scenario.cpp`, and `mapgen.cpp` were used only to locate candidate inherited owners; every conclusion was re-proved below in active YR and retail data.

## COMPLETE evidence and stop conditions

This slice is complete only if all of the following are closed: direct caller/xref and disassembly proof for authored `Read_Scenario -> Full_Init -> ReadMapOverlayPacks -> OverlayClass ctor -> Unlimbo -> virtual Mark`; exact low ID families from retail rules plus Mark comparisons; overlay/data/recalc/Terrain/Techno order; Scenario RNG ownership and order; `.SED` branch exclusivity; synthetic-INI overlay no-op; direct-stamp caller path and absence of Mark/post-stamp pack replay; current Rust source/gate state; and three retail fixtures. Stop/mark PARTIAL if any activation gate, owner, order edge, RNG owner, or source discriminator remains inferred. None does.

## Evidence base

- `[ACTIVE-YR]` Ghidra read-only decompile + disassembly + xrefs: `Read_Scenario 0x00684620`, `Read_Scenario_INI 0x00686730`, `Full_Init 0x00686B20`, `ReadMapOverlayPacks 0x005FD2E0`, `OverlayClass::Constructor 0x005FC380`, `ObjectClass::Unlimbo 0x005F4EC0`, `OverlayClass::Mark 0x005FC570`, `InitMapFromSyntheticINI 0x00599650`, `RandomMapGenerator::Generate 0x00598960`, `PlaceLowBridgeDeck 0x0058F2C0`, `PlaceBridgeRepairHut 0x005904B0`, and `TechnoClass::Constructor 0x006F2B40` (RNG site `0x006F3254`). No metadata was changed.
- `[ACTIVE-YR]` load-bearing xrefs: `ReadMapOverlayPacks` has one call, `Full_Init+0xF14 @ 0x00687A34`; `Full_Init` has calls from ordinary `Read_Scenario_INI @ 0x00686845` and synthetic RMG init @ `0x00599A56`; synthetic init has sole caller `Generate @ 0x00598A74`; direct deck writer has sole call @ `0x005906D5`.
- `[RETAIL-DATA]` extracted retail `rulesmd.ini` at `C:\Users\enok\Documents\ra2-rust-game\ini\rulesmd.ini`; installed loose `Lostlake.mmx`, `Killer.mmx`, `Shrapnel.mmx` decoded read-only with the repository LCW chunk format. All three declare `NewINIFormat=4`.
- `[RUST]` direct read of `src/map/overlay.rs`, `src/map/resolved_terrain.rs`, `src/map/rmg/*`, `src/app/loading/init.rs`, `src/app/frontend/list_maps.rs`, and `src/sim/movement/movement_bridge_retail_tests.rs` in this worktree.

## Verified authored-load timeline

| Order | Active YR event | Exact evidence / consequence |
|---:|---|---|
| 1 | `[ACTIVE-YR]` non-`.SED` source chooses the ordinary reader | `Read_Scenario`: byte `Scenario+0x34BD` is tested @ `0x00684961`; false jumps to `Read_Scenario_INI @ 0x006849C9`, which unconditionally calls `Full_Init @ 0x00686845`. |
| 2 | `[ACTIVE-YR]` map/Iso and explicit tubes precede overlays | `Full_Init` calls `Read_Map_Section_And_IsoMapPacks @ 0x006879FF`, then `ReadTubesINI @ 0x00687A0B`. Tube parsing is a separate mechanism, not low-overlay Mark. |
| 3 | `[ACTIVE-YR]` one overlay owner runs | `ReadMapOverlayPacks @ 0x00687A34`; it returns immediately when `[Basic] NewINIFormat <= 1` (`CMP ... 1/JLE @ 0x005FD2EC..F3`). |
| 4 | `[ACTIVE-YR]` `[OverlayPack]` is traversed deterministically | Fixed 512x512 traversal, `y=0..511` outer and `x=0..511` inner (`0x005FD3F4..0x005FD51C`). Each eligible non-`0xFF` row is handled synchronously before the next coordinate. Textual pack-key order is only compressed-payload assembly; activation order is decoded cell order. |
| 5 | `[ACTIVE-YR]` each accepted packed row reaches virtual `Mark(1)` | Filters require a render image or CellAnim, reject multiplayer crates, and require allocated/in-bounds cell. It allocates 0xB0 and calls `OverlayClass::Constructor @ 0x005FD4D2`; constructor calls `ObjectClass::Unlimbo @ 0x005FC4B1`; Unlimbo dispatches vtable `+0x124` with argument 1 @ `0x005F4FB0..B4`, whose Overlay slot is `Mark @ 0x005FC570`. Terrain objects cannot block this constructor path because `[Terrain]` is read later. |
| 6 | `[ACTIVE-YR]` Mark executes at the packed row, not in a later bridge pass | During `Full_Init`, the load-suppression counter is nonzero: ordinary passability/`Overrides` checks are bypassed, but Mark's universal `SlopeIndex > 4` rejection remains (except unrelated id `0xB2`). A successful ordinary row or every procedural write calls `RecalcAttributes @ 0x0047D2B0` immediately. |
| 7 | `[ACTIVE-YR]` `[OverlayDataPack]` is a second, later 512x512 pass | `0x005FD5F7..0x005FD656`; every allocated/in-bounds cell receives decoded byte `Cell+0x11E` @ `0x005FD640`, including cells generated procedurally or absent/rejected in pass one. Thus data-pack bytes win over Mark-written data. |
| 8 | `[ACTIVE-YR]` global terrain recalculation follows both packs | `Full_Init` iterates all cells and calls `RecalcAttributes @ 0x00687A5A`, after overlay data and before `[Terrain] @ 0x00687A74`. |
| 9 | `[ACTIVE-YR]` authored objects are later and category/entry-ordered | Tiberium growth/spread init @ `0x00687A85/8A`; `[Units] @ 0x00687AA7`, `[Aircraft] @ 0x00687ABF`, `[Infantry] @ 0x00687ACB`, `[Structures] @ 0x00687AEA`, then `[Smudge] @ 0x00687B0E`. Each of the four Techno readers gets section entry count, starts index 0, and calls `GetEntryNameByIndex(section,index)` while incrementing to count. Low Mark cannot occur after any authored Techno constructor in this load. |

## Which bridge rows call Mark

`[RETAIL-DATA]` The stock `[OverlayTypes]` declarations are body keys `77..104=LOBRDG01..28`, endpoint keys `125..128=LOBRDGE1..4`, concrete body keys `209..236=LOBRDB01..28`, and concrete endpoint keys `237..240=LOBRDGB1..4`. Registry insertion is dense; active runtime IDs are proven by Mark's comparisons, not by subtracting one from sparse INI keys.

| Runtime IDs | Retail identities | `[ACTIVE-YR]` behavior when accepted from authored OverlayPack |
|---|---|---|
| `0x4A..0x65` | `LOBRDG01..28` wood bodies/end pieces, including destroyed sinks `0x64/0x65` | Constructor calls Mark once at that packed coordinate. They do **not** enter the procedural endpoint branch; ordinary Mark stores the identity/data then recalculates. |
| `0x7A..0x7D` | `LOBRDGE1..4` wood procedural triggers | Constructor calls Mark; comparisons @ `0x005FC796..A2` select the wood endpoint branch. If its target three-cell endpoint row is empty, Mark writes/recalculates all three, scans for the opposing end, then may materialize body rows. The trigger is not an independent later pass. |
| `0xCD..0xE8` | `LOBRDB01..28` concrete bodies/end pieces | Same ordinary one-row Mark behavior as wood bodies; no procedural expansion. |
| `0xE9..0xEC` | `LOBRDGB1..4` concrete procedural triggers | Comparisons @ `0x005FCBB9..CB` select the concrete endpoint branch with the same boundary conditions. |
| `0x18,0x19,0xED,0xEE` | high bridge anchors (`BRIDGE1/2`, `BRIDGEB1/2`) | High dispatch occurs earlier in the same Mark call and invokes `SetBridgeDirection_*`; these are **not** low bridge variants. Only these four have their prior `+0x11E` saved/restored around construction @ `0x005FD4DB..0x005FD502`. Low rows do not. |

`[ACTIVE-YR]` Because each packed coordinate completes before the next, procedural writes can be observed or overwritten by later non-`0xFF` packed coordinates. Earlier packed coordinates are never revisited. The later full `[OverlayDataPack]` pass then overwrites `+0x11E` globally. Implementing endpoint expansion as a component-wide post-pass is therefore not equivalent.

## RNG boundary and authored Technos

- `[ACTIVE-YR]` Successful low endpoint body materialization draws once for each of the three cross-row body cells at each longitudinal step. Assembly @ `0x005FCB44..70` and `0x005FCF72..9E` loads `ScenarioClass* [0x00A8B230]`, sets `ECX=Scenario+0x218`, calls `Random__Next`, masks `&3`, writes `+0x44/+0x11E`, then recalculates. Empty-row failure, missing opposing endpoint, body-only rows, and high stamps consume zero low-Mark draws.
- `[ACTIVE-YR]` Every Techno constructor later draws unconditionally from that same `Scenario+0x218` stream @ `0x006F3249..59` and stores the low word at `Techno+0x3C8`. `BuildingClass::Constructor` directly reaches this base constructor; Unit/Aircraft/Infantry do through their inheritance path.
- `[ACTIVE-YR]` Therefore fixed-map low-Mark draws advance the shared Scenario cursor before authored Unit -> Aircraft -> Infantry -> Structure constructor words, with each category retaining INI entry-index order. Deferring Mark, using MapGen/Main RNG, batching variants after object construction, or rolling failed Unlimbo back changes every later consumer.

## Mutually exclusive generated `.SED` path

1. `[ACTIVE-YR]` `Read_Scenario` takes the last four filename bytes and compares them case-insensitively with bytes `".SED\0"` at `0x0083DA88`; comparator `0x007C8D20` folds ASCII case. It sets `Scenario+0x34BD=1` @ `0x006846AA`, otherwise zero @ `0x006846BE`.
2. `[ACTIVE-YR]` Random=true calls the seed reader @ `0x00684975`; success calls `RandomMapGenerator::Generate(0,0) @ 0x00684989` and post-map init @ `0x00684990`. Random=false alone calls ordinary `Read_Scenario_INI @ 0x006849C9`. These arms are mutually exclusive.
3. `[ACTIVE-YR]` Generate initializes `g_MapGenRng`, then unconditionally calls `InitMapFromSyntheticINI @ 0x00598A74` **before** water/regions/bridges. For non-preview launch, synthetic init calls `Full_Init @ 0x00599A56`.
4. `[ACTIVE-YR]` That pre-generation Full_Init does not replay overlays: the synthetic INI writes only `[Map]`, `[Basic] Player`, a House `TechLevel`, and `[Lighting]`; it omits `[Basic] NewINIFormat`. `ScenarioClass::Read_INI_Basic` reads that key with default 0 @ `0x0068A13D..56`, so nested `ReadMapOverlayPacks` returns at `0x005FD2F3`. There are no authored Techno section rows either.
5. `[ACTIVE-YR]` Later call graph is `Generate -> BridgeAndConnectorPass 0x0058EF10 -> CarveConnectorsOrBridges 0x005905D0 -> PlaceLowBridgeDeck 0x0058F2C0`. The writer stores complete three-wide overlay rectangles directly to `Cell+0x44` and cross indices to `Cell+0x11E`; its callee list contains no Overlay constructor, Unlimbo, or Mark.
6. `[ACTIVE-YR]` Direct-deck identities are deterministic from coordinates (`EW 0x5E ... 0x5C`, interiors `0x4A+(x%4)`; `NS 0x60 ... 0x62`, interiors `0x53+(y%4)`). Deck search/end coins use `g_MapGenRng @ 0x00ABE890`, not Scenario. Repair-hut placement then calls `BuildingClass::Constructor`, so successful generated CABHUT construction consumes Scenario at the normal Techno base site—but still no low Mark draw.
7. `[ACTIVE-YR]` After direct stamping, Generate performs recalculation/generation tails but never calls Full_Init or ReadMapOverlayPacks again. Xrefs prove the only Full_Init inside Generate's call tree is the earlier synthetic-init call. Thus the completed deck is not input to authored Mark.

### Exact discriminator Rust must retain

`[ACTIVE-YR]` The discriminator is **load provenance: the successful `.SED`/Random branch which generated the live map**, carried across materialization. It is not overlay identity, endpoint/body content, `OverlayDataPack` presence, filename of a serialized in-memory `MapFile`, successful CABHUT count, constructor-trace presence, or a guessed “looks generated” geometry predicate. Generated and authored maps legitimately contain the same body IDs and data.

`[RUST]` Rust already has the right types: `LoadedMapSource::Generated { seed_name }` and `OverlayLoadSource::{Authored, GeneratedMaterialized}`. However, production chooses `GeneratedMaterialized` using `generated_construction_trace.is_some()` in `src/app/loading/init.rs`, an incidental proxy; `MapFile::from_bytes` calls `parse_overlay_packs` regardless of parsed `basic.new_ini_format`; and `ResolvedTerrainGrid` currently gates only `high_bridge_stamp_for_overlay`. No `0x7A..0x7D/0xE9..0xEC` low endpoint implementation exists. The low Mark owner must require authored provenance **and** native `NewINIFormat>1`, share the authored-only source gate with high Mark, and leave generated overlays/data byte-for-byte materialized.

## Retail fixtures and what each proves

- `[RETAIL-DATA]` `Lostlake.mmx`, SHA-256 `39AE274E92A64CA1D5534876DE81DFFAF7153A696900B22A288D3EDB52C81143`: intact wood EW lane `y=117,x=39..51` (three-wide deck; `0x5E` west end, `0x5C` east end). It proves ordinary body/end-piece rows survive authored row-major Mark/data ordering.
- `[RETAIL-DATA]` `Killer.mmx`, SHA-256 `423C7A997D80F964B4490910DA17124EFB3B42D49E14B191BBB281D3AE565845`: intact wood NS deck, lanes `x=93..95,y=130..151` (22 cells; `0x60` north row and `0x62` south row). It supplies the orthogonal body-order/data fixture.
- `[RETAIL-DATA]` `Shrapnel.mmx`, SHA-256 `3D0955DAA3CC146688D88555C6D0938A2E58648DEB25AFF51592EB5D8DAC77E0`: terminal destroyed wood rows `0x64` at `(107,46..48)` and `0x65` at `(114..116,59)`, matching the in-repo movement fixture rectangles `(106..108,46..48)` and `(114..116,58..60)`. These are body-range ordinary Mark rows, never procedural triggers.
- `[RETAIL-DATA]` The decoded packs in all three named fixtures contain zero `0x7A..0x7D`/`0xE9..0xEC` trigger cells. They validate preservation/negative dispatch, not successful trigger expansion; that acceptance case must be a synthetic authored-map fixture using retail rules. The pre-existing 184-map census's zero trigger count is not used to call the executable path dormant.

## Implementation handoff

| # | Behavior -> Rust delta -> surface | Acceptance -> proposed test | Risk if wrong |
|---:|---|---|---|
| 1 | `[ACTIVE-YR]` provenance gates authored Mark -> derive `OverlayLoadSource` directly from `LoadedMapSource`, not construction-trace presence; feed one shared authored-only gate to high and low Mark -> `src/app/loading/init.rs`, `src/map/resolved_terrain.rs` | A generated complete EW/NS deck retains every id/data and consumes zero Mark Scenario words, even with an empty/missing construction trace -> `gsi_04_13_generated_source_never_replays_low_or_high_mark` | Every accepted `.SED` bridge can be corrupted and the match RNG shifted. |
| 2 | `[ACTIVE-YR]` pack Mark requires `NewINIFormat>1`, then low triggers execute inline in decoded row-major order -> suppress pack Mark at missing/0/1, otherwise dispatch at the current overlay loop rather than a component post-pass; body ranges take ordinary Mark only -> `src/map/map_file.rs` or load gate, `src/map/resolved_terrain.rs`, narrow low-Mark owner | Missing/1 formats perform zero pack Mark; two format-4 trigger arrangements prove earlier/later overwrite order and exact draws -> `gsi_04_13_authored_low_mark_requires_format_and_runs_inline_in_pack_order` | Legacy/synthetic inputs activate wrongly, or fixture-dependent topology/variants diverge. |
| 3 | `[ACTIVE-YR]` low writes recalc immediately, then data pack wins, then global recalc -> preserve the three phase boundaries -> resolved terrain/overlay-data application | Synthetic authored endpoints with conflicting data bytes finish with pack data and final Road-derived attributes -> `gsi_04_13_low_mark_data_pack_then_final_recalc_order` | Cross-row frame/land/cache state differs even when ids look right. |
| 4 | `[ACTIVE-YR]` Mark and Techno use one Scenario cursor -> thread the established scenario bootstrap owner into low Mark before authored map-object construction; consume three words per generated longitudinal row and none on rejected/no-op arms -> low-Mark owner + scenario bootstrap/map spawn boundary | Seeded endpoint map followed by Unit/Aircraft/Infantry/Structure rows matches exact variant words and constructor low words in native order -> `gsi_04_13_low_mark_draws_precede_authored_techno_constructor_words` | Essentially every authored match with active endpoints shifts later deterministic RNG. |
| 5 | `[ACTIVE-YR]` body/destroyed rows never expand -> retain exact retail identities/data -> retail ignored tests | Lostlake and Killer retain their complete orthogonal decks; Shrapnel retains both 0x64/0x65 three-cell strips with no extra draw -> `retail_low_mark_preserves_lostlake_killer_and_shrapnel_body_rows` | Common stock low decks or the only destroyed-low loose fixture regress. |

## Negative facts / do not do

- `[NEGATIVE, ACTIVE-YR]` Do not call procedural endpoint support TS-only/dormant merely because the scanned stock-map corpus has zero trigger cells. Retail YR rules declare the types and active YR Mark reaches them from accepted authored content; activity is content-conditional.
- `[NEGATIVE, ACTIVE-YR]` Do not treat `0xED/0xEE` as low; they are high `BRIDGEB1/2` anchors. Do not give low rows the high-only `+0x11E` save/restore gate.
- `[NEGATIVE, ACTIVE-YR]` Do not run a second low-Mark pass after OverlayDataPack, Terrain, Techno construction, or RMG direct stamping.
- `[NEGATIVE, ACTIVE-YR]` Do not infer source from overlay ids/data, `.SED` text after materialization, or construction outcomes. Preserve the branch provenance.
- `[NEGATIVE, ACTIVE-YR]` Do not use MapGen/Main RNG for fixed authored low Mark, or Scenario RNG for deterministic direct deck identities. Do not roll back draws after later placement failure.
- `[NEGATIVE, ACTIVE-YR]` Do not merge explicit `[Tubes]` with low overlay loading; it is read earlier by a separate owner.
- `[NEGATIVE]` Do not port OpenTS `TrainBridgeSet`, generator phases, or load lifetime. OpenTS was navigation evidence only.

## Remaining uncertainty

- `[UNCERTAINTY / intentionally out of scope]` Exact endpoint direction tables, opposing-end termination identity, dummy-cell behavior at each generated coordinate, and variant table bases still belong to the separate low-Mark inner-algorithm investigation. No activation/order/source conclusion here depends on guessing them.
- `[UNCERTAINTY / non-load-bearing]` This pass independently decoded only the three required retail maps, not the earlier 184-map census. The census's zero-trigger statement is treated as a fixture-selection fact, not evidence of dormancy.
- No unresolved activation gate, section order, RNG owner, generated-vs-authored exclusivity, or discriminator remains in this bounded report.

## Stale-document wording to correct

- `ASSET_PARSING_BRIDGES_GHIDRA_REPORT.md` rows calling `0xED/0xEE` “Low bridge variant” are wrong: they are active high `BRIDGEB1/2` Mark anchors.
- `docs/plans/bridge-movement-matrix.md` X-11's “Zero cells ... TS heritage” is too strong. Replace with: “zero cells in the scanned stock loose-map corpus; active YR retail-rules-declared authored-content path, therefore content-conditional, not dormant.”
- Any wording that the `.SED` branch “does not call Full_Init/ReadMapOverlayPacks” is misleading. It calls synthetic `Full_Init` **before generation**; omission/default `NewINIFormat=0` makes the overlay reader inert. The exact negative is: no ordinary file reader in the random arm and no pack/Mark replay after direct stamping.
- [BRIDGE_MAP_LOAD_AND_BRIDGEHEAD_TRANSITIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_MAP_LOAD_AND_BRIDGEHEAD_TRANSITIONS_GHIDRA_REPORT.md)'s low-bridge “deserves a separate pass” caveat is superseded for activation/order by this report; its low inner-algorithm caveat remains valid.

## Annotation candidates (not applied)

- `ReadMapOverlayPacks @ 0x005FD2E0`: `authored_overlay_pack_y_major_x_minor_mark_then_overlay_data_owner`
- `OverlayClass::Mark @ 0x005FC570`: `low_endpoint_triggers_7A_7D_E9_EC_use_scenario_rng_before_techno_sections`
- `ScenarioClass::Full_Init @ 0x00686B20`: `overlay_mark_data_final_recalc_terrain_units_aircraft_infantry_structures_order`
- `RandomMapGenerator::InitMapFromSyntheticINI @ 0x00599650`: `launch_synthetic_basic_defaults_new_ini_format_zero_overlay_reader_inert`
- `ScenarioClass::Read_Scenario @ 0x00684620`: `case_insensitive_sed_provenance_excludes_ordinary_ini_reader`
- `RandomMapGenerator::PlaceLowBridgeDeck @ 0x0058F2C0`: `direct_materialized_low_deck_no_overlay_mark_or_post_stamp_pack_replay`

## Cold checks

1. Re-ran `get_function_xrefs` after forming the timeline: the only `ReadMapOverlayPacks` call remains `Full_Init @ 0x00687A34`, and the only direct writer call remains `CarveConnectorsOrBridges @ 0x005906D5`.
2. Re-read current Rust after forming the discriminator verdict: `LoadedMapSource::Generated` exists, but production still selects `OverlayLoadSource` from `generated_construction_trace.is_some()`, pack parsing ignores `basic.new_ini_format`, and the overlay loop still implements only high anchors.
3. Decoded the three retail packs independently of the pre-existing fixture prose; hashes, `NewINIFormat=4`, both orthogonal intact spans, and Shrapnel's two destroyed terminal strips agree.
