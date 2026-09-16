# Authored Mark Load-Context Source Provenance — Ghidra Research Report

**Date:** 2026-08-31
**Address(es):** `Read_Scenario @ 0x00684620`, `ScenarioClass__Full_Init @ 0x00686B20`, `ReadMapOverlayPacks @ 0x005FD2E0`, `OverlayClass__Mark @ 0x005FC570`
**Investigation Mode:** exhaustive-slice
**Prior-work row:** specific gaps + verification only; this report cold-audits the source/provenance gap left by the high-confidence 2026-08-30 all-context report
**Claimed Scope:** every active-YR fresh, generated, replay, and stream-restore ingress that decides whether the authored OverlayPack transaction can run and which exact `ScenarioClass+0x218` cursor it receives; mapping of that matrix to every current Rust loader surface
**Non-Scope:** low-Mark ID tables and procedural stamp internals, shared-dummy field semantics, OverlayData/Recalc internals, campaign/network transport implementation, FinalAlert, and Rust implementation
**Confidence:** HIGH
**Active in YR:** Conditional — authored fresh loads execute the pack body only when `[Basic] NewINIFormat > 1`; generated `.SED`, stream restore, and shipped-editor cases do not execute authored Mark

## 1. Overview

The active binary does not decide authored Mark from overlay geometry, filename family beyond the `.SED` split, or any later construction record. It decides it from two independent inputs: a fresh-load ingress that reaches `ScenarioClass__Full_Init`, and the value written by that Full_Init's `[Basic] NewINIFormat` read. The exact Mark cursor is a third independent input: the live `ScenarioClass+0x218` state after the selected context prefix and terrain Fill.

The current Rust loader conflates those axes. `LoadedMapSource` already records physical provenance, but `load_map_from_initial` selects `GeneratedMaterialized` from `generated_construction_trace.is_some()`, `ResolvedTerrainGrid` ignores `BasicSection::new_ini_format`, and several loader surfaces lack a typed fresh-load family. A construction trace is downstream Techno-construction evidence; it cannot confer or remove authored-map authority.

### Ownership and transaction routing

| Owner | Exact role in this slice | Status after this report |
|---|---|---|
| GSI-04.13 — low/water bridge topology, decks, ramps, traversal | primary owner of fixed-map low OverlayPack Mark activation and its Scenario cursor | native contract verified; Rust mechanism remains open |
| GSI-04.12 — high-bridge topology, occupancy, traversal | shared authored OverlayPack source/gate/cursor boundary | shared transaction-3 contribution only; aggregate row remains open |
| GSI-04.15 — low-bridge tubes/tunnels and endpoint movement | negative separation: explicit `[Tubes]` loads before OverlayPack; low Mark is not Tube authority | no positive implementation ownership in this transaction |
| BR-M04 — high-bridge map-load stamp and overlay-data order | shared high-load contribution: source/gate/interleaving boundary | open; transaction 4 still owns remaining topology work |
| BR-M05 — low-bridge procedural endpoint/body stamp | direct fixed-authored Mark owner | open until implementation and critic pass |
| BR-M11 — low Road traversal | receives the finalized low Road payload created by the load transaction | open; later mutation-preservation checks still required |

Routing dependencies are GSI-01.07 (`ScenarioClass+0x218`), GSI-02.09 (scenario/packs), GSI-17.01 (fresh load order), GSI-17.04 (stream restore), GSI-17.07 (native replay relaunch), and the applicable campaign/network setup rows. They are dependencies, not substitute owners for GSI-04.12/04.13.

## 2. Key State, Offsets, and Discriminators

| State / discriminator | Exact location | Verified meaning | Evidence | Active in YR? |
|---|---:|---|---|---|
| Scenario RNG | `ScenarioClass+0x218` | one complete Random object used by Fill and authored Mark | Fill receiver `0x004ACFA5`; Mark receivers `0x005FCB52`, `0x005FCF80` | Yes |
| scenario filename | `ScenarioClass+0x125C` | ordinary/replay-selected scenario name copied before `Start_Scenario` | replay read in `Main__PrepareSession/Main_Game @ 0x0052D9A0` | Conditional — replay/fresh launch |
| generated-scenario flag | `ScenarioClass+0x34BD` | set by the case-insensitive terminal `.SED` branch | `Read_Scenario @ 0x00684620`, branch into `0x0068495B..0x00684989` | Conditional — `.SED` only |
| parsed NewINIFormat global | `0x00A8ED7C` | overwritten by each Full_Init's `[Basic]` read; pack-body gate is signed `1 < value` | write `0x0068A156`; gate `0x005FD2EC..0x005FD2F3` | Yes on every Full_Init |
| WOL selector | `0x00A8B244` | exact value `2` chooses common `AssignStartingPoints` in Full_Init | `0x0068755E..0x0068756B` plus writer census | Conditional — compiled WOL state 2 |
| LAN-adjacent state | `0x00A8B24C` | LAN writes `2`; it is not the WOL selector | LAN setup `0x0052E3D2`; xref comparison with `0x00A8B244` | Conditional — LAN/IPX |
| editor byte | `0x00A8ED6B` | cleared persistently at startup; one dormant helper only saves/sets/restores it temporarily | `0x0052F63E`; `0x005A922F`, `0x005A95A1` | No persistent shipped-editor ingress |
| serialized Scenario extent | `0x3740` bytes | raw stream read includes `+0x218`, then native reseeds that RNG with zero | `0x006894AC..0x006894C5`; `0x00683564..0x0068356C` | Conditional — save restore |

No vtable identity claim is needed for this slice. Mode callbacks are cited only as already-cold-audited prefix boundaries; their internal low-Mark tables are deliberately not re-owned here.

## 3. Core Logic

### 3.1 The physical-source split

**Active in YR: Yes.** `Read_Scenario @ 0x00684620` performs a case-insensitive comparison of the final four filename characters with `.SED` (`0x0083DA88`, comparator `0x007C8D20`). A match sets `Scenario+0x34BD`, loads RandomMap options at `0x00597A10`, and calls `RandomMapGenerator::Generate @ 0x00598960` at `0x00684989`. A non-match reaches the ordinary scenario INI reader.

**Active in YR: Yes.** `Read_Scenario_INI @ 0x00686730` constructs one `CCFileClass` for the supplied name, loads it, and on success unconditionally calls `ScenarioClass__Full_Init`; file-load failure returns before Full_Init. The Scenario layer has no Loose-versus-MIX Mark branch. `CCFileClass`/the VFS resolves that physical distinction below it.

**Active in YR: Conditional.** Any successfully opened non-`.SED` scenario is authored for this boundary, including campaign maps and externally supplied `.map`/`.mpr`-style scenarios. Extension or physical container does not identify the fresh prefix family. Current game/session state does.

### 3.2 Full_Init and the exact format gate

The active ordered owner is:

```text
fresh Scenario seed/state
  -> context-specific House / Gather / chooser prefix
  -> ScenarioClass__Read_INI_Basic
       ReadInt("Basic", "NewINIFormat", default=0)
       store 0x00A8ED7C
  -> Read_Map_Section_And_IsoMapPacks / Fill
  -> explicit [Tubes]
  -> ReadMapOverlayPacks
       ClearSectionCache
       if NewINIFormat > 1: decode OverlayPack, construct/Mark in source order,
                            then apply OverlayDataPack
       DrainDeferredFinalizationQueue
  -> whole-map cell Recalc
  -> Terrain
  -> authored Techno sections
```

**Active in YR: Yes.** The relevant Full_Init calls are Fill at `0x006879FF`, explicit Tubes at `0x00687A0B`, the sole `ReadMapOverlayPacks` call at `0x00687A34`, Terrain at `0x00687A74`, and Units at `0x00687AA7`. The helper has no source or game-mode discriminator of its own.

**Active in YR: Yes.** `ScenarioClass__Read_INI_Basic` pushes integer default `0` at `0x0068A13D`, key address `0x0083E128`, section address `0x0082BF9C`, calls `INIClass::ReadInt @ 0x005276D0`, then stores the returned value at `0x0068A156`. Missing `NewINIFormat` cannot inherit a prior scenario's value.

**Active in YR: Conditional.** `ReadMapOverlayPacks @ 0x005FD2E0` always clears the section cache and always drains the deferred-finalization queue, but both OverlayPack and OverlayDataPack bodies are inside the single `1 < 0x00A8ED7C` branch. Missing, negative, `0`, or `1` values therefore consume neither pack as live map state.

### 3.3 Complete ingress/provenance matrix

| Native ingress | Physical/source proof | Fresh context and prefix before Fill | Overlay-pack / exact cursor disposition | Active in YR? |
|---|---|---|---|---|
| stock offline authored, loose-resolved | successful non-`.SED` `CCFileClass` open | disposable House pass -> common `+0x80` -> selected `+0x84` -> zero-draw reset -> final House pass | Mark iff `NewINIFormat>1`; receives same Scenario cursor after prefix and Fill | Yes |
| stock offline authored, MIX-resolved | same ordinary reader; VFS resolves container | same stock-offline family | identical to loose; container does not change cursor | Yes |
| campaign authored | ordinary reader under campaign state | one campaign `[Houses]` construction pass; no multiplayer callbacks and no disposable/final double pass | Mark iff `NewINIFormat>1`; receives campaign post-Fill cursor | Yes |
| LAN/IPX Battle/Coop authored | host/guest network seed; LAN writes game mode and `0x00A8B24C=2`, not WOL selector | disposable House pass -> common `+0x80` Gather -> selected family `+0x84` Gather/chooser -> zero-draw reset -> final House pass | Mark iff `NewINIFormat>1`; receives LAN-family post-Fill cursor | Conditional — LAN/IPX session |
| WOL state `2` authored | WOL options/session seed and `0x00A8B244==2` | disposable House pass -> common `+0x80` -> common `AssignStartingPoints` -> zero-draw reset -> final House pass | Mark iff `NewINIFormat>1`; receives WOL-state-2 post-Fill cursor | Conditional — compiled path; retail service availability is external |
| replay playback of authored scenario | replay header reads seed, scenario name, and session/options fields; then normal RNG init and `Start_Scenario(-1)` | inherits the corresponding recorded campaign/noncampaign family; no replay-only Scenario draw before Start | Mark iff recorded map has `NewINIFormat>1`; receives inherited family's post-Fill cursor | Conditional — replay playback |
| generated `.SED` | explicit terminal suffix; mutually exclusive generator arm | generated launch still executes synthetic Full_Init once, then direct generator materialization | synthetic Basic omits NewINIFormat, so default `0`; helper called but pack body inert; no authored Mark | Conditional — accepted/generated `.SED` |
| arbitrary external/generic non-`.SED` file | successful ordinary `CCFileClass` load proves authored physical state | prefix family comes from current native campaign/game/network state, not extension | Mark iff `NewINIFormat>1`; safe only with the actual native context | Conditional — successfully opened external scenario |
| stream save restore | raw Scenario read through `Load_Game_Content_From_Stream` | no fresh Full_Init/Fill prefix; post-read helper seeds `Scenario+0x218` with zero | no call edge to ordinary reader, Full_Init, pack helper, or Mark | Yes when loading a save |
| persistent `gamemd.exe` editor load | startup forces editor byte off; no persistent enable ingress found | none | excluded; FinalAlert is a different executable | No |

### 3.4 Exact Scenario continuation

**Active in YR: Yes.** Fill's ranged call at `0x004ACFA5` loads ECX from `[0x00A8B230]+0x218`. Both raw Next calls examined in `OverlayClass__Mark` (`0x005FCB52`, `0x005FCF80`) load the same receiver. Full_Init contains no intervening Scenario RNG clone or reseed from the return of Fill through `ReadMapOverlayPacks`.

**Active in YR: Yes.** The correct Rust authority is therefore the complete logical Scenario cursor held after context prefix and all Fill calls. A seed, a counted approximation, a Main RNG cursor, MapGen continuation, or a separately reconstructed Random object is not equivalent.

**Active in YR: Yes.** Mark completes before Terrain/Techno section construction. Any raw low-Mark calls are part of the same stream that later authored Techno constructors consume; moving Mark after object construction changes all downstream words even when final bridge geometry appears equal.

### 3.5 Why construction trace cannot decide generated/no-Mark

**Active in YR: Yes.** Native selects the generated path before generation from the `.SED` suffix and never consults a construction ledger. `RandomMapGenerator__InitMapFromSyntheticINI @ 0x00599650`, called only by Generate, calls Full_Init at `0x00599A56`; its synthetic INI writes map geometry/basic player/lighting fields but no `NewINIFormat`. Later generator phases directly materialize their finished overlays.

**Active in YR: No as a discriminator.** `RmgConstructionTrace` is a Rust-only record of later emitted/discarded Techno constructor events. It is neither an input to native path selection nor evidence that pack bytes originated in authored scenario input. An explicitly generated result remains generated if its trace is empty, absent, stripped by an older caller, or rejected before any Techno event.

**Active in YR: No as a discriminator.** Conversely, attaching trace-like metadata to an authored `LoadedMapSource::Loose` or `::Mix` result cannot turn native ordinary scenario bytes into direct generator state. Provenance must come from the successful loader receipt (`Generated` versus `Loose`/`Mix`), while trace validation remains an orthogonal later construction transaction.

### 3.6 Replay and restore are different ingress classes

**Active in YR: Conditional.** Replay playback reads seven header/session fields in `Main__PrepareSession/Main_Game @ 0x0052D9A0`, initializes the normal fresh RNG state, and calls `Start_Scenario(-1)`. It is a fresh scenario relaunch and can execute authored Mark.

**Active in YR: Yes.** Save restore calls `ScenarioClass__Load_From_Stream @ 0x00689470` only from `Load_Game_Content_From_Stream @ 0x0067E730`. After the `0x3740`-byte raw read, `0x00683564..0x0068356C` explicitly seeds `Scenario+0x218` with `0`. The restore caller's complete callee set contains no fresh scenario reader, Full_Init, pack reader, or Mark. Restore must never be routed through a fresh typed context merely because a snapshot names a map.

The exact internal byte that records every replay mode-family selector is outside this slice; it is not needed for the proven requirement that playback retains/selects the corresponding session family and must not borrow stock-offline by default. This report does not claim that `g_GameMode` itself resides inside the contiguous `0xB8` replay options block.

### 3.7 YR activation, retail data, and OpenTS exclusions

**Active in YR: Yes.** A cold census of 55 installed loose retail map payloads found `NewINIFormat=4` in all 55: 40 `.mmx`, 13 `.yro`, and 2 `.map`. This proves the positive gate is exercised by shipped authored data, but it does not erase the binary's missing/`<=1` custom-content branch.

**Active in YR: Conditional.** Installed `RandMap.Sed` and `SAVE0029.SED` contain RandomMap options only and no `[Basic] NewINIFormat`, matching the synthetic default-zero path.

**Active in YR: No.** `C:\Users\enok\Documents\OpenTS` supplied navigation leads for the inherited `.SED` suffix split, `NewINIFormat` default, overlay gate, and synthetic map-generation initializer. TS-only `Debug_Map`/editor, Biome, VeinholeMonsters, TiberiumWildlife, Firestorm/addon, and inherited editor branches were excluded because active `gamemd.exe` and YR retail data do not establish them as this load ingress.

## 4. Relevant INI/Data Keys

| Section / key | Type and default | Exact effect in this slice | Evidence | Active in YR? |
|---|---|---|---|---|
| `[Basic] NewINIFormat` | signed integer; default `0` on every Full_Init | pack body executes only when value is strictly greater than `1` | read/store `0x0068A13D..0x0068A156`; gate `0x005FD2EC..0x005FD2F3` | Yes |
| `[OverlayPack]` | Base64/LCW packed 512x512 IDs | decoded/constructed in y-major/x-minor order only inside the format gate | `ReadMapOverlayPacks @ 0x005FD2E0` | Conditional — key present and format >1 |
| `[OverlayDataPack]` | Base64/LCW packed 512x512 data bytes | applied after ID construction, also inside the same format gate | `ReadMapOverlayPacks @ 0x005FD2E0` | Conditional — key present and format >1 |
| `[Map] Fill` | map Fill selector; absent/clear behavior is separate | any Fill draws advance the same Scenario object later borrowed by Mark | `Read_Map_Section_And_IsoMapPacks @ 0x004ACE70`, call `0x004ACFA5` | Conditional — selected Fill mode |
| `[Tubes]` | explicit Tube rows | loaded before pack Mark and owned by GSI-04.15, not inferred from low overlay Mark | Full_Init call order `0x00687A0B` before `0x00687A34` | Conditional — rows present |
| `.SED [RandomMap]` options | generator input | selects/guides direct generation; does not supply authored packs or NewINIFormat | retail `.SED` data plus `0x00597A10`/`0x00598960` | Conditional — generated input |

## 5. Integration Points

### Native call/ordering ledger

| Integration point | Verified relationship | Active in YR? |
|---|---|---|
| `Start_Scenario @ 0x00683AB0` -> `Read_Scenario @ 0x00684620` | common fresh scenario filename ingress | Yes |
| `Read_Scenario` -> `.SED` Generate or ordinary reader | mutually exclusive terminal-suffix split | Yes |
| ordinary reader -> Full_Init | unconditional after successful file load | Yes |
| Generate -> synthetic initializer -> Full_Init | one pre-materialization Full_Init at `0x00599A56` | Conditional — `.SED` |
| Full_Init -> pack helper | sole helper caller at `0x00687A34` | Yes |
| Fill -> pack helper -> global Recalc -> Terrain -> Technos | exact ordering; one Scenario cursor crosses the first two owners | Yes |
| replay -> normal RNG init -> Start_Scenario | fresh relaunch, not restore | Conditional — replay |
| stream loader -> seed-zero post-read helper | separate restore path, no pack-reader edge | Yes when restoring |

### Context-prefix distinctions retained from the parent report

The 2026-08-30 parent fully audited the per-family House/Gather/chooser prefixes. This follow-up cold-checked their discriminators and ingress, not their internal low tables:

- **Active in YR: Yes.** Campaign skips the noncampaign callback/double-House branch.
- **Active in YR: Conditional.** LAN/IPX uses common `+0x80` then selected `+0x84`; its adjacent state write is not WOL selector state `2`.
- **Active in YR: Conditional.** WOL selector state `2` uses common `AssignStartingPoints` after `+0x80`.
- **Active in YR: Conditional.** Replay adds no replay-only Scenario draw before normal scenario start and inherits the selected family.
- **Active in YR: Yes.** The first noncampaign House set is deleted by the reset without a Scenario draw; the final House pass is a distinct cursor-advancing pass.

## 6. Current Rust Implementation Status

### 6.1 Existing owners

| Rust surface | Current fact | Provenance sufficiency | Required disposition for this mechanism |
|---|---|---|---|
| `src/app/frontend/list_maps.rs::LoadedMapSource` | exact `Loose`, `Mix`, `Generated`, `LegacyFallback` variants | sufficient for physical provenance except `LegacyFallback` | retain; map `Loose/Mix -> Authored`, `Generated -> GeneratedMaterialized`; reject fallback |
| `src/app/loading/init.rs::MapLoadInitial` | carries map, exact source, MapGen continuation, and optional construction trace | sufficient physical provenance; trace is not source authority | derive overlay origin from `map_source`, never trace presence |
| `src/match_bootstrap.rs::LoadingStartup::Accepted` | owns accepted session and launch seed | sufficient for typed stock-offline context after exact plan validation | construct `StockOffline` fresh context |
| `LoadingStartup::UnverifiedLegacy` | owns a `SkirmishLaunchSession` and seed, but variant name alone does not prove choices resolved | sufficient only when production resolution and `MatchLaunchDescriptor::from_resolved`/prefix-plan validation succeed | accept validated resolved stock-offline session; otherwise reject |
| `LoadingStartup::Generic` | only selected filename; seed is sampled through fallback; no session family | physical source becomes discoverable after loading, but fresh prefix family remains absent | generated/no-Mark and authored format-inactive can be classified; reject authored format-active Mark |
| `src/app/loading/pump.rs::prepare_scenario_prefix_plan` | validates exact `Generated`/`Loose`/`Mix`, rejects `LegacyFallback`, constructs stock-offline plan | sufficient only for current stock-offline family | do not reuse for campaign/LAN/WOL/replay |
| `src/map/basic.rs::BasicSection::new_ini_format` | parses `Option<i32>` | sufficient gate value | use missing as native `0`; runtime gate is `>1` |
| `src/map/map_file.rs` pack parse | pack data is parsed regardless of `new_ini_format` | parsing alone is harmless; current runtime later consumes it | retain as inert metadata if desired, but prohibit live application at `<=1` |
| `src/app/loading/init.rs::load_map_from_initial` | line 1879 selects generated from `generated_construction_trace.is_some()` | insufficient and wrong | use exact `LoadedMapSource`; treat trace orthogonally |
| `src/map/resolved_terrain.rs::OverlayLoadSource` | `Authored` versus `GeneratedMaterialized`; high stamp runs for every Authored input | source enum is useful, but format/context are absent | require authored + format gate + typed exact post-Fill cursor before any Mark |
| `src/sim/scenario_bootstrap.rs::ScenarioBootstrapRng` | owns Scenario/Main/MapGen; exposes ranged Fill adapter | owns correct stream but lacks a post-Fill narrow raw Scenario borrow for Mark | add one ordered nonclone raw continuation seam between Fill and Techno construction |
| `src/headless_scenario.rs::load` | explicit retail path and seed; hardcodes `Authored`; comment admits launch session absent | physical source is discoverably authored, but prefix family is missing | require typed fresh context or reject `NewINIFormat>1` authored load |
| snapshot restore (`GameSnapshot::load_validated`, `restore_after_snapshot_load`) | separate persistence transaction | sufficient restore provenance | never call fresh map/Mark path during restore |
| `src/sim/replay.rs::ReplayRunner` | replays commands into an already-constructed Simulation | not native replay-header scenario ingress | do not claim replay-load parity; a future native replay launch needs typed inherited family |
| shell RMG preview | presentation-only generated map | sufficient generated/non-gameplay provenance | no authored Mark and no gameplay Scenario cursor |
| direct `ResolvedTerrainGrid::build*` callers | several tests/default wrappers choose `Authored` directly | caller can guess authority | make production entry explicit; keep only intentional test fixtures capable of supplying a context |
| inspect-map/render diagnostics | parse/present bytes without constructing gameplay scenario | not a Mark ingress | no fresh context required; must not claim gameplay load parity |

### 6.2 Which current surfaces can construct a typed fresh context

1. `LoadingStartup::Accepted` can construct stock-offline fresh context because it has an accepted launch session and fixed seed.
2. A production `UnverifiedLegacy` can construct the same context only after its session is fully resolved and the existing prefix-plan validation succeeds; unresolved/manual values must reject.
3. No current variant can construct campaign, LAN/IPX, WOL-state-2, or native replay fresh context. Those contexts must remain unsupported rather than borrowing stock-offline.
4. `Generic` and `headless_scenario::load` often can prove the map is authored. They still cannot prove which prefix produced the Mark cursor. The stale description “unknown source” is therefore too broad: the missing fact is context, not necessarily physical source.
5. Snapshot restore has enough provenance to select `Restore`, which is not a fresh context and is always no-Mark.
6. A successful generated `.SED` receipt has enough provenance to select generated-materialized no-Mark even if no construction trace exists.

### 6.3 Bounded typed disposition

| Physical state | `NewINIFormat` | Fresh context | Safe runtime disposition |
|---|---:|---|---|
| Authored (`Loose`/`Mix`/verified external) | missing or `<=1` | absent or present | ignore both packs as live state; no Mark; full-load parity may still require context elsewhere |
| Authored | `>1` | typed stock-offline/campaign/LAN/WOL/replay | consume in native order using exact post-prefix/post-Fill Scenario cursor |
| Authored | `>1` | absent/generic/headless | reject before live application; never guess stock offline |
| Generated materialized | any serialized field value | generated receipt | preserve direct payload; zero authored Mark; trace optionality does not change disposition |
| Restore | any map metadata | restore receipt | bypass fresh builder/Mark entirely |
| LegacyFallback | any | none | reject; no exact parsed-source authority |

### 6.4 Minimum normalized fresh-context contract

This is the smallest verified input/algorithm boundary needed by transaction 3. It does not require implementing campaign UI, LAN/WOL transport, or native replay-file parsing inside the bridge owner. It requires an upstream owner to normalize those facts before a Mark-active authored map is admitted.

Every positive descriptor also carries the common inputs: authoritative 32-bit fresh Scenario seed/state, exact authored `LoadedMapSource`, parsed `NewINIFormat>1`, map/active Scenario waypoint data used by the chosen prefix, ordered House/type inputs used by Full_Init, and exclusive ownership of the one `ScenarioBootstrapRng` until Mark returns its continuation.

| Typed fresh family | Minimum normalized family-specific inputs | Exact algorithm before Fill/Mark | Reject condition | Active in YR? |
|---|---|---|---|---|
| Campaign | explicit campaign-family tag; ordered `[Houses]` construction rows, or the verified registered-HouseType fallback when that section is empty | construct the campaign House set once in native row/fallback order, spending one `RandomRanged(450,1800)` invocation per constructed House; run no multiplayer Gather/chooser and no disposable/reset/final double pass; then Fill and Mark | missing campaign tag or unresolved House construction order | Yes |
| LAN/IPX Battle | network-authoritative host/guest seed; explicit LAN tag; normalized Battle launch slots with human/AI/Special classification; selected active Scenario starts/waypoints; ordered first/final House inputs | disposable House pass -> selected mode common `+0x80` Gather -> Battle `+0x84` Gather/chooser -> zero-draw House/type reset -> final House pass -> Fill -> Mark | missing LAN provenance, unresolved slots/starts, or absent Battle callback family | Conditional — LAN/IPX Battle session |
| LAN/IPX Cooperative | same LAN provenance/seed plus explicit Cooperative family and normalized human-prefix/AI-suffix slot classification | disposable House pass -> common `+0x80` Gather -> Cooperative `+0x84` Gather/chooser -> zero-draw reset -> final House pass -> Fill -> Mark | missing LAN provenance, unresolved Coop slot partition/starts, or absent Cooperative callback family | Conditional — LAN/IPX Cooperative session |
| WOL state `2` | network-authoritative seed; explicit WOL provenance with selector value `2`; normalized player-controlled versus AI House classification; active Scenario starts/waypoints; ordered first/final House inputs | disposable House pass -> common `+0x80` Gather -> common `AssignStartingPoints` (its Gather plus gated player/AI chooser rules) -> zero-draw reset -> final House pass -> Fill -> Mark | WOL selector not proven `2`, unresolved House control class/starts, or an attempt to substitute LAN selected `+0x84` | Conditional — compiled WOL state-2 session |
| Replay | recorded seed and scenario/source; explicit normalized inherited family discriminator; that family's normalized inputs above (campaign, stock-offline, LAN Battle/Coop, or WOL state 2) | initialize fresh Scenario/Main state from recorded seed, add zero replay-specific Scenario calls, then dispatch to the inherited family's exact algorithm -> Fill -> Mark | recorded/inherited family cannot be normalized; never default replay to stock offline | Conditional — replay playback of authored scenario |

Stock-offline is already normalized by the merged P0-R1 `MatchLaunchDescriptor`/active-waypoint/prefix-plan path. Its presence does not make it the fallback for any row above.

#### Mandatory Generic/headless admission rule

After bytes are loaded, `Generic` must inspect the exact `LoadedMapSource`. Headless can classify a successfully loaded explicit **non-`.SED`** retail-file path as authored; a `.SED` name must reject unless it is routed through the explicit generator and receives generated provenance. Then:

1. `LoadedMapSource::Generated` / explicit generated receipt: admit as generated-materialized for this mechanism and execute zero authored Mark, regardless of construction-trace presence.
2. Authored `Loose`/`Mix`/verified explicit file with `new_ini_format.unwrap_or(0) <= 1`: admit only as **Mark-inactive for this mechanism**, ignoring both packed sections as live state. This does not certify the rest of the generic/headless scenario prefix.
3. Authored source with `new_ini_format > 1`: require one of the typed descriptors above or the validated stock-offline descriptor. If absent, return a load error before applying either packed section.
4. `LegacyFallback`, a directly parsed headless `.SED`, or any source/context disagreement: reject. A caller-supplied seed, non-`.SED` filename extension, retail-root path, trace, or “ordinary skirmish” assumption cannot satisfy the descriptor.

The headless API therefore needs a typed context input (or an explicitly Mark-inactive fixture contract); its existing `(retail_dir, map_file_name, seed)` tuple is insufficient for format-active authored maps. The Generic app path must not turn its fallback seed into stock-offline provenance.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `Read_Scenario` terminal `.SED` split | verified | `0x00684620`, `.SED` string `0x0083DA88`, branch/call `0x0068495B..0x00684989` | none |
| ordinary scenario reader success/failure | verified | `Read_Scenario_INI @ 0x00686730` | none |
| Loose versus MIX decision level | verified | ordinary reader plus `CCFileClass`; current `LoadedMapSource` census | none |
| synthetic `.SED` Full_Init | verified | `0x00599650`, call `0x00599A56` | none |
| synthetic NewINIFormat omission/default | verified | synthetic INI writer plus `0x0068A13D..0x0068A156` | none |
| pack helper gate and unconditional bookends | verified | `ReadMapOverlayPacks @ 0x005FD2E0` | none |
| sole pack-helper caller | verified | caller xref to Full_Init `0x00686B20`, call `0x00687A34` | none |
| Full_Init Fill/Tubes/pack/Recalc/Terrain/Techno order | verified | `0x006879FF..0x00687AA7` | none |
| stock-offline ingress/context | verified | P0-R1 report/current Rust plan plus Full_Init callbacks | none |
| campaign ingress/context | verified | Full_Init campaign branch and 2026-08-30 parent | none |
| LAN/IPX ingress/context discriminator | verified | `0x0052E3C6`, `0x0052E3D2`, `0x00687558..0x0068757B` | none |
| WOL-state-2 ingress/context discriminator | verified | `0x00A8B244` writer census, `0x0068755E..0x0068756B` | none |
| replay fresh relaunch | verified | `Main__PrepareSession/Main_Game @ 0x0052D9A0` | none |
| stream restore raw read/seed-zero/no-Mark | verified | `0x0067E730`, `0x00689470`, `0x00683564..0x0068356C`, callee census | none |
| Fill and Mark receiver identity | verified | `0x004ACFA5`, `0x005FCB52`, `0x005FCF80` | none |
| persistent shipped editor ingress | verified negative | `0x0052F63E`; temporary writes `0x005A922F`, `0x005A95A1` | none |
| retail authored format census | verified | 55 installed payloads; all `NewINIFormat=4` | none |
| retail `.SED` key census | verified | `RandMap.Sed`, `SAVE0029.SED` | none |
| OpenTS navigation/exclusion | verified as non-authority | OpenTS scenario/overlay/mapgen leads checked against gamemd/YR data | none |
| current Rust physical provenance surfaces | verified | `list_maps.rs`, `loading/init.rs`, `loading/pump.rs` | none |
| current Rust format consumption | verified mismatch | `basic.rs`, `map_file.rs`, `resolved_terrain.rs` | none |
| current Rust fresh-context variants | verified mismatch | `match_bootstrap.rs::LoadingStartup` | none |
| current Rust headless/generic discoverability | verified mismatch | `headless_scenario.rs`, `generic_map_load` callsites | none |
| current Rust persistence/replay separation | verified | `app/persistence/mod.rs`, `sim/snapshot.rs`, `sim/replay.rs` | none |

No target-scope item is `touched-not-exhausted`, `not-touched`, `deferred`, or `conflict-needs-resolution`.

## 8. Open Questions — Final State of the Investigation Log

- `[RESOLVED] OQ-01 — Can ReadMapOverlayPacks run outside Full_Init? -> No; Full_Init is its sole caller.` (evidence: caller xref `0x005FD2E0 <- 0x00686B20`)
- `[RESOLVED] OQ-02 — What exact predicate activates both packed sections? -> Signed integer value strictly greater than 1.` (evidence: `0x005FD2EC..0x005FD2F3`)
- `[RESOLVED] OQ-03 — Can missing NewINIFormat inherit the preceding map's value? -> No; every Full_Init reads with default 0 and overwrites the global.` (evidence: `0x0068A13D..0x0068A156`)
- `[RESOLVED] OQ-04 — Are OverlayPack and OverlayDataPack gated separately? -> No; both bodies are inside the same outer branch.` (evidence: complete `0x005FD2E0` body)
- `[RESOLVED] OQ-05 — Does the helper itself inspect filename/source/mode? -> No.` (evidence: complete `0x005FD2E0` body)
- `[RESOLVED] OQ-06 — What makes a fresh file generated rather than authored? -> Case-insensitive terminal `.SED` comparison before the ordinary reader.` (evidence: `Read_Scenario @ 0x00684620`)
- `[RESOLVED] OQ-07 — Does generated `.SED` skip Full_Init? -> No; synthetic initializer calls it at 0x00599A56.` (evidence: `0x00599650`, disassembly `0x00599A4B..0x00599A5B`)
- `[RESOLVED] OQ-08 — Why does synthetic Full_Init not execute authored Mark? -> Synthetic Basic omits NewINIFormat, whose native default is 0.` (evidence: synthetic writer and `0x0068A13D..0x0068A156`)
- `[RESOLVED] OQ-09 — Can construction-trace presence prove generated source? -> No; native decides before generation and has no such input.` (evidence: `Read_Scenario @ 0x00684620`; current Rust `loading/init.rs:1879` mismatch)
- `[RESOLVED] OQ-10 — Does an empty/missing generated trace authorize authored Mark? -> No; explicit successful generated provenance remains no-Mark.` (evidence: `.SED` branch and direct generator path)
- `[RESOLVED] OQ-11 — Does a trace-like artifact attached to authored bytes change native ingress? -> No.` (evidence: ordinary reader/source split)
- `[RESOLVED] OQ-12 — Is Loose versus MIX a different native Mark family? -> No; source resolution is below the ordinary scenario reader.` (evidence: `Read_Scenario_INI @ 0x00686730`)
- `[RESOLVED] OQ-13 — Do filename extensions identify campaign/LAN/WOL prefix family? -> No; active globals/session state select the Full_Init family.` (evidence: Full_Init campaign/mode branches)
- `[RESOLVED] OQ-14 — What cursor does stock-offline Mark receive? -> Same Scenario object after two-House/two-callback prefix and Fill.` (evidence: P0-R1 report; `0x004ACFA5`, `0x005FCB52`, `0x005FCF80`)
- `[RESOLVED] OQ-15 — What cursor does campaign Mark receive? -> Campaign single-House-pass continuation after Fill.` (evidence: Full_Init campaign branch; parent report)
- `[RESOLVED] OQ-16 — Does LAN use WOL state-2 common assignment? -> No; LAN writes adjacent 0x00A8B24C, then uses selected +0x84.` (evidence: `0x0052E3D2`, `0x0068755E..0x0068757B`)
- `[RESOLVED] OQ-17 — What is WOL state-2's second callback? -> Common AssignStartingPoints after common +0x80.` (evidence: `0x00687558..0x0068756B`)
- `[RESOLVED] OQ-18 — Is replay equivalent to restore? -> No; replay initializes fresh RNG then calls Start_Scenario.` (evidence: `Main__PrepareSession/Main_Game @ 0x0052D9A0`)
- `[RESOLVED] OQ-19 — Does replay add a Scenario draw before the inherited prefix? -> No replay-specific draw was found between normal RNG init and Start_Scenario.` (evidence: `0x0052D9A0` path)
- `[RESOLVED] OQ-20 — Does stream restore run Full_Init/pack Mark? -> No.` (evidence: `Load_Game_Content_From_Stream @ 0x0067E730` callee census)
- `[RESOLVED] OQ-21 — What Scenario RNG state follows native stream read? -> Explicit seed-zero state.` (evidence: `0x00683564..0x0068356C`)
- `[RESOLVED] OQ-22 — Is there a persistent editor scenario ingress in shipped gamemd? -> No; startup clears the flag and the only other writer restores it.` (evidence: `0x0052F63E`, `0x005A91E0`)
- `[RESOLVED] OQ-23 — Does current Rust retain NewINIFormat? -> Yes, as Option<i32>, but runtime overlay application ignores it.` (evidence: `src/map/basic.rs:32`, `src/map/resolved_terrain.rs`)
- `[RESOLVED] OQ-24 — Which current Rust value incorrectly selects generated materialization? -> generated_construction_trace.is_some().` (evidence: `src/app/loading/init.rs:1879..1883`)
- `[RESOLVED] OQ-25 — Is current Generic physical source unknowable? -> Not after load; MapLoadInitial records Loose/Mix/Generated, but Generic still lacks a prefix family.` (evidence: `LoadedMapSource`, `generic_map_load`, `load_initial`)
- `[RESOLVED] OQ-26 — Is headless source unknowable? -> No; it joins the supplied filename to the retail root and is discoverably authored, but lacks launch session/prefix.` (evidence: `src/headless_scenario.rs:150..163` and module scope comment)
- `[RESOLVED] OQ-27 — Which current variants can represent campaign/LAN/WOL/replay fresh context? -> None.` (evidence: `src/match_bootstrap.rs:80..88`)
- `[RESOLVED] OQ-28 — Can snapshot restore reuse a fresh context? -> No; Rust persistence is already a separate validated restore transaction.` (evidence: `src/app/persistence/mod.rs`, `src/sim/snapshot.rs`)
- `[RESOLVED] OQ-29 — Are shipped authored maps known to exercise the positive gate? -> Yes; all 55 inspected payloads declare 4.` (evidence: retail-data census)
- `[RESOLVED] OQ-30 — Did the final zero-add pass reveal another ingress? -> No; re-reading the four primary owners/caller sets and the full Rust loader census added zero entries.` (evidence: 2026-08-31 zero-add pass)

There are zero deferred and zero open entries.

## 9. Exhaustion and Adversarial Cases

### Zero-add and cold spot-checks

1. Cold spot-check A re-decompiled `ReadMapOverlayPacks @ 0x005FD2E0` and re-read `0x0068A13D..0x0068A156`. It reconfirmed one outer `>1` gate and a per-Full_Init default-zero overwrite.
2. Cold spot-check B re-read `.SED` synthetic call `0x00599A56`, the restore seed-zero instructions `0x00683564..0x0068356C`, and the restore caller callee set. It reconfirmed “synthetic Full_Init but inert pack body” and “restore without fresh reader.”
3. The final ingress/Rust-owner census added zero new entry points or unresolved questions.

### Five adversarial cases

| Case | Evidence-backed answer | Active in YR? |
|---|---|---|
| authored format-4 load followed by generated `.SED` | synthetic Full_Init overwrites NewINIFormat with 0; no stale positive gate | Conditional — sequence can occur across launches |
| generated result with empty/absent construction trace | remains generated-materialized and executes zero authored Mark | Conditional — generated ingress |
| authored Loose/Mix map with missing, 0, or 1 format | both packed sections are inert as live state; zero Mark | Conditional — custom/legacy authored data |
| generic/headless explicit authored format-4 map | physical source is known, but cursor family is not; Rust must reject rather than guess offline | Current Rust-facing active parity case |
| replay of authored map versus save restore of same map | replay fresh-relaunches and may Mark; restore never enters the helper and seeds Scenario zero | Conditional — respective ingress |

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| physical source and fresh context are independent | `Read_Scenario`, Full_Init mode branches | conflated at load application | `LoadedMapSource`, `LoadingStartup`, approved `FreshScenarioLoadContextDescriptor` boundary | carry exact source plus a separate typed fresh family | `loose_and_mix_sources_remain_authored_even_with_trace_like_artifact` | do not infer family from extension or source container |
| generated source is explicit and no-Mark even without trace | `.SED` split; `0x00599A56`; default 0 | generated chosen by trace presence | `load_map_from_initial`, `MapLoadInitial` | derive generated-materialized from `LoadedMapSource::Generated`; validate trace later | `generated_source_without_construction_trace_never_runs_authored_mark` | do not make trace optionality a semantic switch |
| authored pack consumption requires strict `NewINIFormat>1` | `0x0068A13D..56`, `0x005FD2EC..F3` | parser retains value; runtime ignores it | `MapFile`/`ResolvedTerrainGrid` load boundary | missing/negative/0/1 leave both pack ID and data payload inert | `authored_overlay_pack_gate_uses_new_ini_format_and_ignores_missing_or_one` | do not use `>=1` or `>=4`; do not apply OverlayData alone |
| Mark receives exact post-prefix/post-Fill Scenario object | `0x004ACFA5`, `0x005FCB52`, `0x005FCF80` | only ranged Fill adapter exposed; Mark seam absent | `ScenarioBootstrapRng`, loading terrain construction | borrow one nonclone raw Scenario continuation after Fill and return it for later constructors | `campaign_lan_wol_replay_contexts_continue_exact_post_fill_scenario_state_into_mark` | do not seed/reconstruct, use Main/MapGen, or batch Mark later |
| campaign/LAN/WOL/replay have distinct prefix families | Full_Init `0x0068745E..0x0068757B`; replay entry | no typed variants | startup/context descriptor boundary | expose only when actual entry point has the corresponding provenance; otherwise explicit unsupported | same fixture set with full logical-state checkpoints per family | do not let absent context borrow stock-offline |
| generic/headless authored Mark-active loads lack context, not source | current source/path code plus native mode selection | both may enter Authored without prefix | `generic_map_load`, `headless_scenario::load`, direct terrain builders | reject format-active authored application until typed context supplied | `generic_or_headless_mark_active_authored_load_requires_typed_fresh_context` | do not call all explicit filenames “unknown source”; do not silently accept |
| generated `.SED` synthetic Full_Init defaults to zero then directly materializes | `0x00599A56`, synthetic INI, retail `.SED` | generated payload is preserved, but authority proxy is wrong | generated launch path and terrain builder | preserve every direct overlay/data byte with zero authored Mark | `generated_sed_preserves_direct_overlay_payload_with_new_ini_zero` | do not discard generated payload merely because format is 0; gate applies to authored pack replay |
| restore is a separate no-Mark transaction | `0x0067E730`, `0x00689470`, `0x00683564..6C` | Rust persistence already separate | snapshot/persistence integration tests | preserve separation and verify no fresh builder invocation | `snapshot_restore_never_enters_full_init_fill_or_overlay_mark_and_seeds_scenario_zero` | do not re-run map initialization from saved filename |
| shipped authored corpus exercises positive gate | 55-payload census | no corpus gate test | retail integration fixture owner | prove at least one Loose and one MIX/archived authored format-4 load reaches typed gate | `retail_format4_authored_sources_enter_mark_gate` | corpus evidence does not permit deleting <=1 branch |

The context-family acceptance bundle must include these concrete normalized-input tests:

- `campaign_mark_context_runs_one_house_pass_without_multiplayer_callbacks`, covering both ordered `[Houses]` input and the empty-section registered-type fallback;
- `lan_battle_mark_context_runs_common_then_selected_callbacks_before_final_houses`;
- `lan_coop_mark_context_preserves_human_prefix_ai_suffix_chooser_family`;
- `wol_state2_mark_context_uses_common_assign_and_rejects_lan_selected_callback`;
- `replay_mark_context_dispatches_recorded_family_without_extra_draw_and_rejects_unknown_family`;
- `generic_and_headless_format_active_authored_loads_require_typed_context`;
- `generic_format_inactive_authored_load_skips_both_packs_without_inventing_stock_offline_context`.

### Negative Facts

- **Active in YR: No.** Construction-trace presence is not a native source discriminator.
- **Active in YR: No.** Filename extension/container identifies neither campaign nor LAN/WOL/replay prefix family.
- **Active in YR: No.** Save restore does not call Full_Init, Fill, `ReadMapOverlayPacks`, or Mark.
- **Active in YR: No.** Generated `.SED` does not skip Full_Init; the correct negative is that its synthetic Full_Init leaves the pack body inert.
- **Active in YR: No.** Shipped `gamemd.exe` has no persistent editor scenario-load mode.
- **Active in YR: No.** LAN's write of `0x00A8B24C=2` does not select WOL's `0x00A8B244==2` branch.
- **Active in YR: No.** Low OverlayPack Mark is not explicit TubeClass loading; `[Tubes]` precedes it.
- **Active in YR: No.** `NewINIFormat=4` in the shipped corpus is not the binary threshold; the threshold is strictly greater than 1.

### Remaining Uncertainty

No material uncertainty remains inside the claimed ingress/provenance/cursor slice.

Bounded non-claims:

- exact replay-header byte naming beyond the seed/scenario/options fields needed for this ingress proof;
- external WOL service operability in 2026, which does not change the compiled state-2 branch;
- low-Mark internal ID tables, dummy-cell mutations, and final stamp formulas, owned by the other transaction-3 reports;
- the future Rust API spelling beyond the approved typed-context architectural boundary.

### Stale Docs / Follow-up Docs — exact replacement wording

Replace any claim that generated `.SED` has no Full_Init with:

> Accepted `.SED` generation enters `ScenarioClass::Full_Init` once through `RandomMapGenerator::InitMapFromSyntheticINI @ 0x00599650` (call `0x00599A56`). The synthetic INI omits `[Basic] NewINIFormat`, and `ScenarioClass__Read_INI_Basic @ 0x0068A156` writes default `0`, so `ReadMapOverlayPacks @ 0x005FD2E0` is called but its pack body is skipped. Later generated overlay/deck state is written directly; no authored Mark transaction occurs.

Replace any claim that construction-trace presence selects generated/no-Mark with:

> Explicit successful `.SED` / `LoadedMapSource::Generated` provenance selects generated-materialized no-Mark. `RmgConstructionTrace` is orthogonal later Techno-construction evidence; missing or empty trace does not turn generated state into authored OverlayPack input.

Replace any claim that generic/headless necessarily has unknown physical source with:

> Generic/headless explicit file loads are often discoverably authored from actual `Loose`/`Mix`/path provenance, but they lack the native fresh prefix family. They may not enter Mark-active authored loading until a typed context is supplied.

## 11. Ghidra Annotation Candidates

Read-only worker: no metadata was changed or saved.

| Address/source | Current metadata | Proposed metadata | Kind | Live proof | Status |
|---|---|---|---|---|---|
| `0x0068A156` | instruction in Basic reader | `NewINIFormat ReadInt(default=0) overwrites global every Full_Init` | EOL comment | argument pushes `0x0068A13D..0x0068A149`, store at `0x0068A156` | worker-report-only |
| `0x00599A56` | call inside synthetic initializer | `.SED synthetic initializer calls Full_Init; omitted NewINIFormat makes pack body inert` | EOL comment | call target `0x00686B20`, synthetic INI field census | worker-report-only |
| `0x00684989` | Generate call in `.SED` arm | `terminal .SED path uses generator and excludes ordinary scenario reader` | EOL comment | suffix branch and callsite | worker-report-only |
| `0x00683564` | post-stream-read helper instruction | `stream restore reseeds Scenario+0x218 with zero; no fresh Mark path` | EOL comment | push 0, receiver `+0x218`, restore caller | worker-report-only |
| `0x0068755E` | Full_Init mode discriminator | `WOL selector state 2 chooses common AssignStartingPoints; LAN uses adjacent state and selected +0x84` | EOL comment | global xref census and branch targets | worker-report-only |

## Sources

- Live active-retail `gamemd.exe` in Ghidra: `0x004ACE70`, `0x0052D9A0`, `0x0052E3C6`, `0x0052E3D2`, `0x00597A10`, `0x00598960`, `0x00599650`, `0x005FC570`, `0x005FD2E0`, `0x0067E730`, `0x00683560`, `0x00684620`, `0x00686730`, `0x00686B20`, `0x00689470`, `0x0068A156` and cited instruction ranges.
- `docs/research/bridges/01-assets-map-load-overlay/LOW_OVERLAY_MARK_ALL_LOAD_CONTEXT_SCENARIO_RNG_LIFECYCLE_GHIDRA_REPORT.md`.
- `docs/research/bridges/01-assets-map-load-overlay/LOW_OVERLAY_MARK_SCENARIO_LOAD_ACTIVATION_BOUNDARY_GHIDRA_REPORT.md`.
- `docs/research/bridges/01-assets-map-load-overlay/LOW_OVERLAY_MARK_FIXED_MAP_STAMP_RNG_TRANSACTION_GHIDRA_REPORT.md`.
- [docs/research/bridges/00-system-models/SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md).
- `docs/plans/2026-08-28-active-retail-bridge-parity-design.md` and current living inventory.
- Current Rust owners on merged P0-R1 ancestry: `src/app/frontend/list_maps.rs`, `src/app/loading/init.rs`, `src/app/loading/pump.rs`, `src/match_bootstrap.rs`, `src/map/basic.rs`, `src/map/map_file.rs`, `src/map/resolved_terrain.rs`, `src/sim/scenario_bootstrap.rs`, `src/headless_scenario.rs`, `src/app/persistence/mod.rs`, `src/sim/snapshot.rs`, `src/sim/replay.rs`.
- Installed YR retail map/`.SED` data census described above.
- `C:\Users\enok\Documents\OpenTS` scenario/overlay/mapgen sources as navigation leads only.

## Final Status

**COMPLETE for the exact source/provenance/first-Mark-cursor slice.** The Open Questions Log is drained, the zero-add pass added nothing, five adversarial cases are answered, and two cold Ghidra spot-checks agreed. Implementation remains open under transaction 3 / BR-M04-shared, BR-M05, and BR-M11 and must pass its builder/fresh-critic loop.
