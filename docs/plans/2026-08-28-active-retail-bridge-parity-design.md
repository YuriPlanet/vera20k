# Active-Retail Bridge Parity Architecture Design

## Goal

Parity-close VERA20k's complete active-retail Yuri's Revenge bridge system, including GSI-04.12 high bridges, GSI-04.13 low/water bridges, GSI-04.14 destruction/repair/CABHUT behavior, GSI-04.15 TubeClass, and every active cross-system bridge consumer established by the bounded coverage baseline and its living follow-up inventory.

The result must preserve behavior that already matches, replace behavior that conflicts with active `gamemd.exe` or retail data, implement missing behavior, and leave no approximate, unchecked, missing, or residual bridge mechanism behind.

## Status and Scope Decision

**Revision 24 is TRANSACTION-3-SLICE-D-LANDED. PR #216 (`feature/bridge-queue-rebuild-parity`) carries OQ-38, the native tiberium queue stores: the per-`TiberiumClass` entry array and 1-based float min-heap with the native sift-up/sift-down and `(MapRect.Height + 4) * MapRect.Width * 2` capacity, rebuilds in `CellIterator` order, the x87 chop batch product and signed attempts arithmetic, the literal `OverlayData < 0x0B` reinsert, the `CellClass+0xE4 FirstObject` occupier gate over ground Technos plus every terrain object, the class-free `CanSpreadTiberium` admission that lets `Reduce_Tiberium` queue foreign-class neighbours into the removed class's store, and a snapshot restore that rebuilds from the serialized `[Map] Size` rect. Its critic chain is critic 1 NEEDS_FIX on `80e41172`, critic 2 NEEDS_FIX on `3142c09f`, and critic 3 PASS on `51b51a3a`, with seven residuals recorded. PR #213 (`feature/bridge-generated-germination`, on `main` @ `4f780edb`) carries the generator tail for random maps: growth-then-spread queue initialization from the painted densities, the final `InitCellAttributes(1)` density rewrite through the shared `SpreadCellGerminate(0)` model, the resource presentation refresh, and the crate Mark seam's native neighbour order. Its critic chain is critic 1 PASS on `e203f975` (live decompiles through the plugin's HTTP endpoints) with three residuals recorded in `71666d51`. PR #211 (`feature/bridge-post-load-tail`, on `main` @ `c06e4f65`) carries the post-`Full_Init` setup tail: the GasCloudSys particle-system native ID on every fresh load (spent inside the synthetic `Full_Init` on generated launches), the `[General] OreTwinkle` / `[AudioVisual] OreTwinkleChance` native readers, one Scenario `RandomRanged(0, chance-1)` draw per resource cell in `CellIterator` order with a `TWNK1` Anim on every zero roll, the `HideIfNoOre` `AnimClass::AI` consumer, the signed `RandomRanged` bounds, and the value-only `MapClass+0x134` aggregate stored by the authored final sweep. Its critic chain is critic 1 NEEDS_FIX on `e2e44865`, critic 2 PASS on `792a32ce` with its one residual (the `HideIfNoOre` block position) applied verbatim in `b535249e`. PR #207 (`feature/bridge-authored-overlay-finalization`, merged with `main` @ `15a48e55`) carries the fresh authored load corridor: the one y-outer/x-inner OverlayPack/OverlayData transaction with the ephemeral Overlay object lifecycle, ordinary/high/low/wall Mark, Land-5 germination, the unconditional drain, the first anti-diagonal Recalc with per-Mark/first-sweep terrain Anims, the consumed-once finalized identity/state/authored-wall-count payload, Terrain/growth-then-spread-queue/Techno/Smudge native-ID ordering, scalar deletion and the final unlatch/Recalc sweep, the synchronous wall-mutation host, and both production ingresses (`load_map_from_initial` authored arm and `headless_scenario::load`). Its critic chain is wall critic 1 NEEDS_FIX on `95f77159`, wall critic 2 PASS on `da38da27`, and full-slice critic 3 PASS on the merged HEAD with zero blocking findings and six recorded residuals (below). Transaction 3 remains the active transaction: G10 (generated phase journal), G11 (preview lifetime), the ancillary `InitCellAttributes` slot seam, the `None` retained-plane rejection gate, the CellAnim tiberium remap/ZAdjust child fields, and the `FUN_00586BF0` bridge-record restamp (routed to transactions 4/13) stay open, so no `BR-M` row closes here.**

`docs/research/bridges/00-system-models/ACTIVE_RETAIL_BRIDGE_COVERAGE_REINVESTIGATION_GHIDRA_REPORT.md` at commit `50d0ef8a` is the bounded discovery baseline. Ten successive read-only omission audits expanded that baseline to 27 mechanism rows; the living inventory now has 38 explicit questions (OQ-37 was opened by PR #207's fresh critic, OQ-38 by transaction-3 slice C). OQ-34 closed the complete pre-map native-ID prefix, while the same zero-add pass reopened and then routed the terminal raw `0x100000`/`0x200000` clear/restamp and ordinary-cell LightConvert recomputation as OQ-35/OQ-36. The tenth broad pass added nothing. Before every transaction the inventory is refreshed against then-current `main`, active-retail `gamemd.exe`, retail data, named validation, and critic evidence. A newly proved writer, consumer, contradiction, or exclusion reopens the affected mechanism and transaction routing. Status comes from cited evidence and commits, never a hand-maintained parity percentage.

The fourth review's random-map blocker and the Revision-4/Revision-5 critics' launch, projection,
trace-schema and authored-constructor blockers are closed by
`docs/research/bridges/00-system-models/RMG_BRIDGE_DUAL_RNG_LIFECYCLE_REINVESTIGATION_GHIDRA_REPORT.md`
at commits `7fee6929`, `4a63fa15`, `f1e6054b`, and `a776f270`. It proves that preview and gameplay
are separate generation runs, that every generator entry reconstructs MapGen from the stored map
seed, and that RMG Techno construction advances the caller's Scenario stream even for later-failed
placement attempts. Preview uses the process shell Scenario cursor; successful Start replaces
Scenario/Main from the match seed before the `.SED` reader invokes the generator again. The launch
generator nests `Full_Init` before its CABHUT/neutral-Techno attempts, so one match-seeded Scenario
owner must span preload, Fill, the ordered attempt trace, projection and Simulation. Generated low
decks are already fully stamped and never pass through fixed-map `OverlayClass::Mark`. It also
proves the dormant RMG helper exclusions, the fixed authored-Techno constructor order, the active
Post-Map starting-force paths, and the class-level constructor invariant that covers every runtime
Techno creation ingress. The stored constructor word is later read for report-sound selection, so
it needs a persistent owner. Those generator-phase findings remain closed; the separately discovered
pre-Fill House/Gather prefix and non-offline Mark-entry contexts are routed below.

The design treats those rows as a coverage map, not as implementation modules. It uses 21 narrower
implementation transactions to preserve dependency order. Each transaction is its own branch,
builder, critic, validation, PR, and merge gate; the 27 `BR-M` rows remain the cumulative behavior-
closure gates. A transaction may supply part of more than one mechanism, and a mechanism may span
more than one transaction; neither fact permits a mechanism to inherit another row's pass.

Current implementation baseline is freshly fetched `origin/main` commit
`15a48e5528` (2026-09-01, PR #209 crate authority) plus PR #207's authored load corridor, PR #211's
post-`Full_Init` setup tail, and PR #213's generator-tail germination,
with PR #170 merge `f4baff6e` confirmed as an ancestor. PR #170 merged P0, transaction 1 (load inputs/raw facts), and
transaction 2 (RMG low-bridge launch construction), including their focused validation and critic
corrections. PR #196 then merged P0-R1's universal stock-offline Scenario prefix, accepted-RMG
active-waypoint ownership, one post-Fill cursor, and its fresh critic corrections. Later merged work
also changed ground-height ownership, Drive/Ship slope handling, radar-overlay projection, FNPC
bridge projection, final House `BasePlan`/AI state, snapshot schema v113, lifecycle/deploy seams, and
bridge-harness hashes. PR #197 added only Team-AI INI tests and did not touch a transaction-3 owner.
PR #207 landed transaction 3's authored OverlayPack, procedural low Mark, finalized-payload, and
two-Recalc-boundary work for authored sources; main's later crate authority (PR #209) is merged
underneath it with distinct snapshot/hash schema steps (v114 crate slots, v115 retained wall plane and
shared-dummy overlay word). Those changes are evidence to preserve, not proof that any remaining
`BR-M` row is closed. Before each remaining transaction is
contracted, its builder runs a fresh direct disparity scan against then-current `main`; stale Rust
gap descriptions in the bounded coverage report are historical hypotheses only.

Living status at this baseline is explicit. P0, P0-R1, and transactions 1 and 2 are merged. BR-M02
and BR-M03 passed their bounded PR #170 gates but remain subject to the final reverse audit. BR-M01,
BR-M06, and BR-M24 have landed contributions but remain open at their routed later consumer or
reverse-audit gates. Transaction 3 is active: its nine completed 2026-08-31 native reports close the exact
source/context gate, synchronous high/low/ordinary traversal, dummy field surface, ordinary
tiberium germination, OverlayData overwrite, authored pre/post-object animation lifecycle, and
generated staged Recalc/animation/resource finalization, including the active preview-native
Building/Anim/ID/sound lifetime across replacement, Cancel, re-entry, and accepted launch, plus the
real authored Overlay object/registry/deferred-drain/slope-survivor lifecycle. A later focused wall
report additionally proves authored ScenarioInit success, active-retail wall reachability, and the
retained blocker-neighbor count plane that final identities cannot reconstruct. They also
reopen Revision 15's one-sweep and dummy-Land/zone/cache assumptions. PR #207 delivered the
authored contributions of G1-G9, G12, and G13 through a passed fresh critic chain; the remaining
transaction-3 work is the residual ledger below (G10, G11, `MapClass+0x134`, ancillary slots, the
`None`-plane gate, CellAnim child fields, and OQ-37). GSI-04.12, GSI-04.13, GSI-04.14, and GSI-04.15 remain
aggregate-open until all of their mechanisms and cross-system consumers pass. This ledger is
replaced with current evidence after each merged transaction rather than appended as historical
prose.

The merged P0-R1 correction replaced Revision 9's eligible/ineligible split. Current `main` now
reproduces the active stock-offline first disposable House pass, both active mode Gather callbacks,
zero-draw House/type reset, second final House pass, and one exact cursor handoff before terrain Fill.
The follow-up all-context audit proves the remaining transaction-3 boundary: campaign uses a single
`[Houses]`-driven pass; LAN and WOL retain both House
passes plus common `+0x80`, with LAN using selected `+0x84` and WOL state `2` using common
`AssignStartingPoints` as the second Gather/chooser; replay inherits its recorded campaign/
noncampaign family; stream restore and generated `.SED` do not run Mark; and shipped
`gamemd.exe` has no persistent editor load mode. Transaction 3 must use this typed matrix rather
than allowing any non-offline context to inherit the stock-offline plan by analogy.

The first House pass is disposable except for its Scenario cursor effect. It cannot overwrite or
leak into the final House `BasePlan`, AI activation latches, lifecycle state, or snapshot/hash state.
The zero-draw reset deletes that pass, and only the second pass supplies final House state. A focused
oracle must continue past installation through Fill, any applicable Mark/generated replay,
authored/Post-Map work, and the current runtime `recalc_base_plan` Scenario consumer so the new
BasePlan path proves both state preservation and downstream cursor continuity.

In scope:

- active stock, mode-active, and content-conditional retail YR bridge mechanisms;
- valid custom-map `[Tubes]` content accepted by the active executable;
- active retail random-map low bridge and CABHUT production;
- cross-system consumers in movement, combat, targeting, terrain/resource placement, triggers, presentation, and persistence;
- evidence-backed negative boundaries needed to keep TS-only, dormant, editor-only, or name-only mechanisms out.

Out of scope:

- OpenTS behavior without independent active-retail proof;
- TS `TrainBridgeSet`, Mech, DropPod, subterranean and inactive LaserFence/meteor branches;
- editor-only tube writing and map-editing UI;
- malformed sentinel-less tube crash fidelity;
- RMG waterfall shaping as bridge topology;
- WaterBridge TMP names, Golden Gate scenery names, refinery radio “bridge” terminology, and generic tag bookkeeping as physical bridge mechanisms.

An evidence-backed exclusion is part of closure. It prevents a later implementation from importing a plausible but non-retail OpenTS or asset-name behavior.

## Architecture Context

The repository already has the correct broad dependency direction, but bridge authority is distributed and several consumers reconstruct the wrong fact locally.

### Immutable and load-time inputs

- `src/map/theater.rs` owns theater bridge-piece inputs.
- `src/map/bridge_facts.rs` owns raw structural cell facts and anchor/ramp classifications.
- `src/map/resolved_terrain.rs` projects TMP, overlay, level and bridge facts into resolved cells.
- `src/map/tube_facts.rs` and `src/map/tubes.rs` own explicit tube data.
- `src/map/rmg/` owns separately seeded MapGen decisions and emits an ordered
  `RmgConstructionTrace` containing every actual constructed CABHUT and every Neutral-Techno
  construction, including discarded Neutral-Tech placement failures. A failed CABHUT site search
  occurs before construction and emits no trace event. The shell-preview or gameplay-load caller
  applies that trace to its own Scenario RNG.

These modules may feed simulation, but they must not depend on it. Low Road overlays, structural high-bridge flags, and TubeClass records remain separate inputs.

### Deterministic simulation owners

- `src/sim/bridge_state/` owns mutable bridge state, endpoint records and damage/repair projection.
- `src/sim/map/bridge_topology.rs` exposes simulation-facing cell bridge views.
- `src/sim/occupancy.rs` and entity `BridgeOccupancy` state own ground/deck lists and persistent `OnBridge` identity.
- `src/sim/pathfinding/` owns entry, A*, zones, hierarchy and post-A* smoothing.
- `src/sim/movement/` owns locomotor transitions, path markers, tubes, scatter and blocked-unit behavior.
- `src/sim/combat/` owns target acquisition, fire gates, AoE admission, selected-plane damage, Rocker admission and bridge-object tolerance.
- `src/sim/world/mod.rs::projectile_collides_at`, `src/sim/projectile.rs::projectile_bridge_crossing`, and the caller in `src/sim/world/techno_ai.rs` own the already-matching projectile bridge-plane collision path that unit 18 must preserve.
- `src/sim/rocking/` owns persistent rocking impulse/timer state; combat must supply its exact native enumeration and dispatch inputs rather than bypassing it with a presentation effect.
- `src/sim/world/bridge_orchestrator.rs` coordinates collapse, repair, CABHUT outputs and deterministic side effects.
- spawn, production, landing, teleport, trigger and terrain-resource owners remain in their existing domains and consume the same bridge facts.

No one Boolean is authority for all of these. Native behavior intentionally distinguishes raw structural `0x100`, transition `0x200`, destroyed/inactive `0x400`, orientation `0x800`, sparkle suppression `0x1000`, mutable state, effective cell height, walkability, persistent object `OnBridge`, ground/deck occupant lists, and TubeClass direction 8.

### Presentation and persistence owners

- `src/render/` and `src/app/presentation/` derive terrain, object split, shadow/railing, PixelFX, action-line, radar and audio commands from immutable simulation snapshots and map assets.
- snapshot/hash owners serialize authoritative deterministic state and rebuild derived topology after load.

Presentation must never repair missing simulation authority by inventing a second bridge model. Conversely, render-only rules such as `TooBigToFitUnderBridge` must not leak into movement legality.

### Current System Map fit

The System Map's `bridge-helpers` service already describes bridge topology/deck predicates feeding traversal, occupancy, area damage, visibility and drawing. This work extends verified connections across existing owners; it does not justify a new cross-layer service or a dependency reversal. System Map entries should change only when an implemented, verified connection changes the mapped graph.

## Design Forces

1. **Exact facts are plural.** Raw bits, mutable bridge state, effective height, object plane and occupant list cannot be collapsed safely.
2. **Native transactions cross modules.** Load stamps, movement relayers, collapse, repair and post-load rebuild must preserve their native order even though Rust owners are separated.
3. **RNG instances are authority.** Scenario, main and MapGen streams cannot be substituted for one another, and fixed draws such as DBRIS `RandomRanged(1,1)` still advance state.
4. **Content frequency does not erase active behavior.** Explicit tubes and campaign trigger actions remain required when the active loader/runtime accepts them.
5. **Presentation has exact raw-fact consumers.** PixelFX and action-line height use raw flags in states where generalized walkability/deck predicates may disagree.
6. **Existing correct behavior is an asset.** Bullet bridge-plane crossing, tactical inverse search, TIBTRE `0x500` refusal and other matches need preservation tests, not gratuitous rewrites.
7. **The closure process is part of correctness.** A builder's own tests cannot close a mechanism without a fresh read-only critic pass.

## Approaches Considered

### A. Distributed exact-delta closure in current owners — chosen

Keep raw inputs, mutable topology, occupancy, pathing, movement, combat, presentation and persistence in their present architectural domains. For each closure unit, establish an implementation contract, correct the smallest owner set needed for the native transaction, add exact positive and negative fixtures, and run the builder/critic loop before proceeding.

Cross-owner consistency comes from explicit facts and transaction inputs, not from moving all bridge code into one module. Shared queries expose raw structural state, mutable state, effective level and object plane separately. Coordinators may sequence a native transaction, but they do not become universal policy owners.

This approach minimizes architecture drift and makes preservation of already-correct paths explicit.

### B. Central `BridgeService` monolith

Move bridge loading, topology, movement, combat, rendering and persistence decisions behind one new service.

This looks attractive because it centralizes terminology, but it would reverse current dependency boundaries, mix immutable map data with mutable simulation and presentation, and encourage a single generalized “has bridge” answer where native needs multiple independent facts. It would also make unrelated movement/combat/render code depend on a subsystem-shaped facade rather than their established domain owners. Rejected.

### C. Literal OpenTS/native-class-shaped port

Recreate OpenTS/Westwood `MapClass`, `CellClass`, `FootClass`, `TubeClass` and bridge walkers as a parallel class hierarchy, then route Rust through it.

Readable inherited code would speed copying, but OpenTS mixes TS behavior, reconstructed choices and names that do not match active YR. A parallel hierarchy would duplicate current Rust state, create synchronization hazards, and import excluded `TrainBridgeSet`/legacy behavior. Rejected as both parity and architecture risk.

## Chosen Architecture

### 1. Preserve separate authoritative facts

The following concepts remain separately queryable and testable:

- raw cell flag word and named masks;
- tile index, subtile, level, overlay ID/data and anchor relation;
- mutable bridge state and endpoint/connection records;
- resolved low Road land/passability;
- explicit TubeClass identity, endpoints, direction path and length;
- persistent entity `OnBridge` state;
- ground/deck object lists and occupation bytes;
- derived zone/hierarchy/path-marker state;
- render-only bridge/depth decisions.

No closure unit may replace one with another unless active native evidence proves equivalence at that exact consumer.

### 2. Use existing domain owners with narrow shared views

`map` continues to own decoded/load-time facts. `sim::bridge_state` owns mutable bridge topology. Occupancy and movement own object-plane transitions. Pathfinding owns reachability and smoothing. Combat owns target/damage plane decisions. Presentation owns draw construction from immutable snapshots.

Where a consumer currently receives an impoverished Boolean, extend that consumer's input with the smallest explicit view it needs. Examples include effective level plus raw structural state for smoothing, selected occupant plane for Rocker, and raw `0x1000` rather than walkability for PixelFX. Do not add a generic omniscient bridge context passed everywhere.

### 3. Model native transaction boundaries explicitly

#### RMG generation transaction

1. On first setup entry only, if `MapSeed+0x74 == -1`, draw the seed from the process shell Scenario
   RNG. Keep Randomize and derived-option draws on Main RNG. Neither path uses MapGen.
2. Each Generate call begins with a fresh MapGen object seeded from `MapSeed+0x74`; it never starts
   from the prior preview continuation. The pure geometry job returns its map payload, MapGen
   continuation and a consumed-once phase-aware extension of `RmgConstructionTrace`. That transport
   retains invocation-specific staged state, generator cell deltas, every native Recalc/
   InitCellAttributes boundary, and stable-ordinal Building records, rather than only final cells or
   a flat entity list. Launch includes the staged synthetic-Full_Init branch; preview does not.
   Each Building record is an actual native constructor in generator order and
   contains phase (`BridgeRepairHut` or
   `NeutralTech`), type identity, and a phase-constrained outcome. Neutral-Tech construction may be
   `Discarded` or `Emitted { entity_index, cell }`; a discarded event carries no invented cell
   because construction precedes its placement loop. CABHUT site search precedes construction:
   a failed search emits no event or constructor effect, while a constructed stock CABHUT is
   `Emitted`. Successful output rows alone are not a sufficient reconstruction because they omit
   discarded Neutral-Tech constructors.
3. Preview consumes the complete phase-aware transport against a process/shell-owned
   `PreviewNativeLifecycle` (or equivalent) that outlives the dialog presentation candidate. Preview
   passes argument `1` and takes `ScenarioClass::Set_Defaults` plus manual map setup; it never takes
   launch `Full_Init`, `Clear_Scene`, or the map-read `+10,000` reservation. Because
   `g_GameActive == 1`, preview Buildings and tile Anims are ordinary active registered objects:
   Unlimbo, Display/Logic/Anim registration, cell latches, and admitted sounds all run. On every
   Generate, first free the prior spread queue and then the growth queue, then set the independent
   native numeric-ID counter to `1,000,000` *before* replacement cleanup without rewinding the shell
   Scenario RNG. An exact storage-key match performs no setup `AssignUniqueID`: its first new object
   can receive `1,000,001` while retained Type, House, Super, real/dummy Cell, Anim, and other
   untouched Abstract objects keep colliding numeric IDs on separate collision-free handles. A
   missing/changed key consumes the exact wrapping prefix
   `R(W,H) + |P_preview| + HB(H_preview,S_preview) + K_preview`, where
   `R(W,H)=H*(2W-1)+1`, `HB(H,S)=H*(1+S)`, and active retail has `K_preview=0`
   because all 176 theater rows/20 distinct TileAnim names already exist in `[Animations]`. From that
   proved post-setup cursor, every actual Building constructor consumes
   one raw Scenario word followed by one preincremented `native_unique_id`; each eligible Recalc Anim
   consumes/registers its native ID before an optional custom RandomRate Scenario draw, while active
   stock TileAnim still consumes an ID/registration/sound lifecycle with zero RandomRate. Failed
   CABHUT pre-searches consume nothing; constructed-but-discarded Neutral-Tech objects consume both
   effects but bind no generated entity. Collision-free Rust runtime handles remain separate from
   native numeric IDs and may not reject or skip a duplicate native value.
4. Replacement uses only the existing four-field normalized width/height/theater/player-count storage
   key. A missing snapshot or changed key performs full old-object/Anim/sound cleanup after the ID
   reset, then Resize constructs every real Size-diamond Cell in row-major order plus the shared dummy
   Cell last, then replays the source-ordered first-new ID-bearing Type events, House/Super blocks, and
   theater allocation arm before later preview objects. A matching key skips all four prefix families
   and consumes zero new Cell IDs; it selectively deletes Unit/Infantry/Building/Terrain but retains
   old final terrain Anims, latches, and sounds through intermediate generator
   Recalcs; newly eligible unlatch cells may create new Anims, so reset numeric IDs can temporarily
   duplicate IDs still held by old Anims. Terminal `InitCellAttributes(1)` scalar-deletes marked
   old/new Anims in live registry order, releases their handles/latches, and recreates the final set
   in the proved anti-diagonal order. Preview preserves CABHUT-before-first-Recalc, every later
   generator Recalc, pre-final growth-then-spread queue initialization, and terminal argument-1
   processing; those final queues persist across Cancel/no-Generate re-entry, and the next Generate
   replaces them only through free-spread-then-growth followed by rebuild-growth-then-spread;
   final cells cannot substitute for that journal. Each Generate still resets MapGen from the current
   map seed while continuing the already-advanced shell Scenario cursor. The shell must choose and
   apply the reset/full-or-selective cleanup branch *before* generation, then pass a consumed-once
   `PreviewGenerationPrestate` containing the exact reused-or-rebuilt Cell/native-ID prefix, retained
   latch state, live marked-Anim registry order, and
   a lifecycle generation token into the Recalc producer. A worker may mutate a local staged latch
   shadow and return an ordered journal only if apply validates that exact token/prestate; a clean-map
   worker followed by post-hoc event suppression is invalid because old latches change intermediate
   Anim eligibility.
5. Use Map with a valid preview performs no third generation. Common teardown writes `RandMap.img`
   and destroys only the preview surface and storage snapshot. Cancel commits neither `.SED` nor the
   sentinel and preserves the registered preview Buildings/Anims, latches, admitted sounds, advanced
   native-ID counter, final growth/spread queues, and returned shell Scenario cursor. Re-entry without Generate changes none of
   that state; the first later Generate resets the counter, observes the missing snapshot, full-cleans
   the old preview state, then generates. Accepted result `1` writes `RandMap.Sed`, rebuilds the
   chooser presentation, commits the ordinary sentinel, and likewise leaves the accepted native
   preview state live until a later real launch cleanup.
6. The accepted preview map, its MapGen continuation, and its live shell-native objects are never
   gameplay map authority. Accepted `.SED` launch first takes the generator-entry free-spread then
   free-growth pair, then `Full_Init`/`Clear_Scene` frees spread then growth a second time while
   destroying preview state, and the later Full_Init tail rebuilds growth then spread for gameplay.
   Cancel takes none of these launch frees. Successful Start has exactly one *logical* gameplay Scenario stream
   beginning at the match seed. Before terrain Fill, every active stock offline noncampaign mode
   performs the complete two-House-pass/two-Gather transaction defined below. Rust may partially
   evaluate that transaction into one immutable `PreFillScenarioPrefixPlan`, including default-cell
   deficient-start retries, only under the full-state equivalence proof below. `LoadingRequest`
   retains the consumed-once plan and `MapLoadInitial` constructs the sole downstream
   `ScenarioBootstrapRng` from the same match seed before adopting its validated continuation. Before
   the first fresh-Full_Init constructor or Fill draw, app consumes that bootstrap once into a staged
   `SimulationLoad`/equivalent: the one real `Simulation` owner with an initially empty map/object
   stage but the authoritative Scenario cursor, native-ID cursor, collision-free handle allocator,
   registries, sound state, queues, and descriptor. It is completed in place as terrain and objects
   arrive; no later `into_simulation` transfer may create a second or shadow live-registry owner.
7. Every fresh native `Full_Init` owns one native-ID cursor spanning the whole load. `Clear_Scene`
   seeds it at `1,000,000`. Let `R(W,H)=H*(2W-1)+1` be every real Size-diamond Cell plus the dummy,
   `HB(H,S)=H*(1+S)` be one House followed by every current Super, and `P` be the complete post-reset
   source-ordered first-new ID-bearing Type event stream including lazy Weapon/Bullet allocations but
   excluding ParticleType. Cold active stock first builds and retains the exact 1,070-event startup
   Type-registry state; its event vector predates the Scenario cursor reset and is drained, not added
   to the fresh-load formula. `E_campaign` is the optional early campaign companion/sidecar Rules
   first-new stream; `E_multi` is the early noncampaign Countries -> General -> live HouseType-body
   first-new delta against the current pre-reset registries (51 events on stock startup state).
   `P_preview` is the corresponding actual successful first-new Type
   stream reached only by a missing/changed preview rebuild. All four symbols are ordered source- and
   prestate-dependent constructor-event streams, never counts inferred from final registries. With
   wrapping dword addition, campaign snapshots
   `1_000_000 + |E_campaign| + |P| + HB(Hc,S1) + R2`; authored noncampaign and accepted `.SED`
   snapshots `1_000_000 + |E_multi| + HB(H1,S0) + R1 + |P| + HB(H2,S1) + R2`.
   The retail explicit-list subtotal is 1,699 actual constructors from 1,704 ID-bearing rows, not a
   seed and not a substitute for `P`. Map read snapshots that exact `C_saved`; custom theater
   allocations after the snapshot are real shadowed events, but active retail's 176 rows/20 names add
   zero, and the later write installs wrapping `C_saved + 0x2710` **from the snapshot**, not by adding
   to the then-current cursor. Every successfully allocated `[Tubes]` source row constructs/spends one
   native ID before parsing; a malformed allocated row spends then hard-fails, and allocation failure
   spends zero then hard-fails. Successful rows bind a consumed-once `TubeNativeInit`. Preview never
   imports this prefix. `.SED` reader success then runs a second
   complete generator call from the stored map seed. Rust continues the one logical Scenario stream
   through the complete pre-Fill prefix and terrain Fill inside that already-live staged Simulation,
   then consumes transaction 3's phase-aware launch transport there. Map remains the geometry/Recalc
   owner and exposes narrow staged operations; the orchestrator invokes sim-owned Building/Anim
   constructors, live registries, sound events, one independent native-ID counter, and one
   collision-free runtime-handle allocator at the corresponding boundary. Actual Building events and
   generated Recalc animations interleave on those shared load owners before final projection; no
   precomputed parallel animation owner is permitted. Final stage completion installs the terrain/
   overlay payload without moving or reconstructing those owners, so Post-Map and gameplay consumers
   continue the same Scenario cursor and registries. No independently
   seeded parallel authority, unchecked cursor substitution, duplicated ID/handle allocator, or
   second downstream continuation is permitted. `Clear_Scene` leaves the shared deferred queue empty,
   and no Type, House/Super, Cell, TagType, or Tube prefix constructor writes it; the pack reader starts
   from `[]` while still using the shared—not Overlay-private—queue and drain.
8. Launch replay builds a `GeneratedTechnoInitTable` keyed by stable generated entity index. A
   successful event stores both the consumed low word as `techno_ctor_random_word` and the already
   allocated Building `native_unique_id`; a discarded Neutral-Tech event consumes both effects but
   has no binding. `MapLoadInitial` carries the consumed-once transport; the load orchestrator builds
   the table during staged consumption and hands the completed table to projection.
   `spawn_from_map_with_resolved` validates entity index, type and cell identity, installs that word
   and native ID on `GameEntity`, allocates/uses its collision-free runtime handle independently, and
   performs no second Scenario draw or *native* ID allocation. Fixed authored-map, Post-Map, and
   runtime Technos use prerequisite P0's fresh constructor-draw path plus their native ID/handle
   owners; snapshot restore reinstalls serialized authoritative identity without constructor effects.
   The constructor word and native identity participate in deterministic snapshots and hashes where
   native persistence requires them.
9. Shell preview generation creates no gameplay bridge records, zones, or entity/cell occupancy.
   That negative boundary does not erase its proved live process-shell Building/Anim registrations,
   terrain latches, native IDs, or sound effects. Launch generation feeds the ordinary gameplay load
   transaction only after its independent native constructor sequence has been accounted for exactly.
10. Preserve `BuildRiverBridge @ 0x0059E740` as waterfall terrain shaping and prove by a negative
   characterization that it writes no runtime bridge overlay/flag topology.

Transaction 3 must preserve the merged unit-2 construction records while extending their transport
with these interface roles (names may change only if the critic can map an equally explicit owner
one-for-one):

```rust
struct RmgConstructionTrace {
    records: Vec<RmgPhaseRecord>,
}

struct RmgPhaseRecord {
    ordinal: u32,
    effect: RmgPhaseEffect,
}

enum RmgPhaseEffect {
    ApplyCellDeltas(RmgCellDeltaBatch),
    Building(RmgConstructionEvent),
    DrainDeferredFinalizationQueue,
    Recalc(RmgRecalcBoundary),
    InitializeTiberiumQueues,
    FinalInitCellAttributes { germinate_and_sum: bool },
}

struct RmgConstructionEvent {
    phase: RmgConstructionPhase, // BridgeRepairHut | NeutralTech
    techno_type: TechnoTypeId,
    // Discarded is valid only for a constructed NeutralTech placement failure.
    // Constructed BridgeRepairHut events are Emitted; failed pre-searches are absent.
    outcome: RmgConstructionOutcome, // Discarded | Emitted { entity_index, cell }
}

enum RmgRecalcBoundary {
    SyntheticFullInit,
    PostBridgeCabHut,
    PostTiberiumFirst,
    PostTiberiumSecond,
    LatHelper,
    GeneratorFinal,
}

struct GeneratedTechnoInit {
    entity_index: usize,
    techno_type: TechnoTypeId,
    cell: CellCoord,
    techno_ctor_random_word: u16,
    building_native_unique_id: u32,
}
```

Every live Building/Anim record also carries an independently allocated collision-free runtime
handle. `native_unique_id` is a reproduced Scenario-counter value, not a collection key: preview may
temporarily hold Type, House, Super, real/dummy Cell, Anim, Building, and other live records with the
same numeric native ID. A process-owned preview lifecycle record retains the native counter, retained
cross-class numeric IDs, live registered Building/Anim records, terrain latches, sound-handle lifetime,
and final growth/spread queues across dialog teardown; the presentation candidate and four-field
storage snapshot remain separate owners and may be destroyed while that lifecycle remains live.

`LoadingRequest` owns exactly one consumed-once `PreFillScenarioPrefixPlan` for every valid active
stock offline noncampaign launch. There is no valid no-plan fallthrough and no Battle/FFA
terrain-independence eligibility gate. The plan is a partial evaluation of this exact native
transaction on the one match-seeded Scenario stream:

```text
S0
  -> House pass 1: H * RandomRanged(450, 1800)
  -> selected-mode vtable +0x80 Gather/preassignment
  -> selected-mode vtable +0x84 Gather/assignment/chooser
  -> rules/type reset and deletion of every pass-1 House (zero draws)
  -> House pass 2: H * RandomRanged(450, 1800)
S1
  -> Fill_In_Data
```

`H` is the same ordered set in both passes: every human node including observers, every valid AI
slot, then Neutral and Special. Stock mode IDs `1, 2, 4, 5, 6, 7, 8, 9` use the Battle family and
ID `3` uses Cooperative; both families execute the `+0x80` and `+0x84` Gather calls. Each deficient
Gather retry consumes exactly two ranged Scenario draws, Y then X, with no retry cap. It evaluates
the already-resized but otherwise default `CellClass` state: clear cells, overlay `-1`, level `0`,
and no occupier, before Fill, Iso, overlays, Terrain, or Technos. A narrow pre-Fill view may therefore
derive bounds from the verified Size/LocalSize inputs and answer only those default-cell predicates;
it must not consult later resolved terrain, overlays, occupancy, or bridge state. Sparse preassigned
start entries below the target count remain in order and the plan retains both Gather vectors and the
final assignment/chooser result rather than collapsing them into one completed vector.

App setup normalizes the constructor roster before compact launch-session projection; it cannot be
reconstructed from `opponents.len()` or from the one-local-human `SkirmishLaunchSession` shape. The
simulation-facing value has the following semantic fields (exact Rust names may follow existing
style):

```rust
struct PreFillHouseRoster {
    human_nodes_in_native_priority_order: Vec<PreFillHumanHouse>, // observers retained
    ai_slots_in_native_slot_order: Vec<PreFillAiHouseSlot>,       // validity retained
    neutral: PreFillHouse,
    special: PreFillHouse,
}
```

Both passes traverse that same immutable value. Each human node consumes regardless of observer
status; only valid AI slots construct and consume; invalid AI slots remain represented so a compacted
opponent vector cannot silently change order or count. Neutral and Special are always last. Final
House `BasePlan`, AI latches, lifecycle fields, snapshots, and hashes are installed only from the
second pass; first-pass outcomes may be retained in the plan for cursor/equivalence proof but are
never projected into the live world.

The accepted generated-map path is not a filename/content inference. A valid preview writes start
staging in `Scenario + 0x11C0`; successful Start copies that staging before `.SED` regeneration.
Setup acceptance extracts only those staged start entries plus explicit
`AcceptedRmgStartStaging` provenance into a consumed-once loading value. Preview terrain, MapGen
continuation, constructor bindings, and display entities remain presentation/file artifacts and are
not carried as gameplay authority. Loading consumes the staged-start value exactly once when making
the prefix plan, which retains a raw active-Scenario waypoint table independently of both Gather
vectors. Random-map loading markers and the live `ScenarioDescriptor` start table read this retained
copy; `.SED` regeneration may supply the launch map and playfield bounds but cannot retroactively
replace the active starts already copied by Full Init. A cancelled/replaced preview, authored map,
fresh external `.SED`, regenerated waypoint inference, construction-trace presence, or unsupported
generated/mode combination cannot manufacture or borrow this provenance. This gate is independent
of the later generated-vs-authored overlay load-source discriminator.

The plan contains the complete logical pre-state/fingerprint `S0`, the retained raw active-Scenario
waypoint table, pass-1 House outcomes for equivalence checking only, both Gather outcomes, final
assignments, pass-2 House outcomes as the sole live-House projection source, and complete
post-state/cursor `S1`.
`MapLoadInitial` creates one `ScenarioBootstrapRng` from the same match seed, requires exact full-state
equality with `S0`, installs `S1` once, and rejects a second installation because the pre-state no
longer matches. The plan exposes no RNG interface and is consumed from `LoadingRequest`; later House
and assignment installation is draw-free. After installation the bootstrap owner alone supplies
Fill, fixed-map low Mark or generated-construction replay as applicable, Post-Map, and simulation
draws. This is the same transition as direct execution, not a second gameplay RNG.

P0-R1's focused tests compare an independent reference stream across both H-sized House passes, both
mode-family callbacks, zero/one/many deficient retries, the zero-draw reset, Fill, RMG emitted and
discarded constructors, Post-Map, and simulation. They must include observers, AI/Neutral/Special,
Cooperative, sparse input, duplicate-install rejection, tampered pre-state, and accepted generated
staging provenance. PR #196's fresh critics rechecked this universal transaction,
single-installation, and downstream-owner model; transaction 3 must preserve those merged tests and
cursor contracts.

`MapLoadInitial` also carries the generated-source identity, `RmgConstructionTrace`, and then the validated generated-init table.
`GameEntity` owns the installed constructor word. Authored structure upgrades are distinct
`GameEntity` owners linked by stable parent ID and upgrade slot. The stable generated entity index
plus type/cell tuple makes a stale or misordered RMG binding a load error rather than silently
spending another draw.

The two event phases are exhaustive for the active generator. `0x005A6510` and `0x005A82E0` are
reachable only from no-xref `0x005A5020`; `0x005A91E0` also has no caller, and none of the three
entry addresses occurs as an image function pointer. Unit 2 must preserve that evidence-backed
exclusion rather than widening the trace for dormant RMG-shaped code.

#### Scenario-load transaction

1. After a fixed map is selected, parse it through the normal scenario loader. For `.SED`, first read
   seed/options and run the launch generator inside the load path after the successful-Start RNG reset;
   do not substitute the accepted preview payload.
2. For a fresh authored load, initialize the complete Scenario state from the authoritative launch
   seed before `Start_Scenario`, then select exactly one typed pre-Fill context:
   - stock offline noncampaign: P0-R1's two House passes, selected `+0x80`, selected `+0x84`, and
     zero-draw reset;
   - campaign: one constructor invocation per `[Houses]` row (or every registered HouseType when the
     section is empty), no multiplayer Gather/chooser and no second pass;
   - LAN/IPX: network seed -> House pass 1 -> common `+0x80` Gather -> selected Battle or
     Cooperative `+0x84` second Gather/chooser -> zero-draw reset -> identical House pass 2;
   - WOL state `2`: network seed -> House pass 1 -> common `+0x80` Gather -> common
     `AssignStartingPoints` (second Gather plus only the zero-occupied player and exactly-two-occupied
     AI chooser draws) -> zero-draw reset -> identical House pass 2;
   - replay: recorded seed/session and the corresponding campaign/noncampaign family, with no
     replay-only draw.
   Fill consumes from and returns the same cursor. A stream restore is a separate no-Full_Init/no-Mark
   transaction and retains native seed-zero Scenario restore behavior; it never enters this list.
3. A fresh Full_Init has one independent native-ID cursor beginning at Clear_Scene's `1,000,000` and
   advancing through the exact campaign or two-pass noncampaign formula in the RMG transaction above:
   actual first-new Type events, House-then-Super blocks, row-major real Cells, and dummy Cell last for
   every reached Resize. Map read snapshots its resulting `C_saved`, retains any custom theater
   `ShadowedAssign` events, then installs wrapping `C_saved + 0x2710` from the snapshot. It parses
   explicit `[Tubes]` after Fill and before overlays; every successfully allocated source row constructs
   and consumes one ID before token parsing and stores it in a consumed-once `TubeNativeInit` keyed to
   that exact parsed Tube record. A malformed allocated row spends the ID and then hard-errors; an
   allocation-null row spends none and hard-errors. Native has no reject-and-continue path. Transaction
   5 must validate/install each successful binding without a second native-ID allocation.
   `MapFile` already retains the full raw `IniFile`, whose `[Tubes]` values preserve first-insertion
   source order, even though its convenience `explicit_tubes: Vec<TubeFact>` filters malformed rows.
   Transaction 3 must consume that retained raw section exactly once through the constructor/
   allocation/Assign/parse boundary and must not use `explicit_tubes` as native-ID-accounting input.
   The validated fact plus `TubeNativeInit` binding is the downstream output.
   Transaction 3 promotes only this exact pre-map/Tubes constructor-ID prefix needed by the first
   Overlay/Anim ID oracle; positive Tube topology, hierarchy, traversal, and persistence stay in
   transaction 5. Keep Tube rows independent of low Road behavior and classify automatic same-cell
   shells from final theater land data without synthesizing traversal. On a successful load with `T`
   constructed Tube source rows, the first reader-admitted, successfully allocated Overlay is exactly
   `O1 = wrap32(C_saved + 10_000 + T + 1)`. Every synchronous CellAnim/terrain-Anim child advances the
   same cursor before the next decoded Overlay; no post-hoc child allocation may preserve that oracle.
4. On an authored source, native signed `NewINIFormat > 1` permits both pack bodies. Execute one
   decoded OverlayPack traversal, `y=0..511` outer then `x=0..511` inner. Apply the native admission
   sequence in order: a successfully read non-`0xFF` identity; type image or non-null CellAnim;
   nonzero-game-mode crate rejection; exact radar-diamond admission; and `0xB0` allocation. A null
   allocation consumes no handle, native ID, registry, Mark, or queue effect; the high-owner restore
   check remains a no-op. For every allocated row, construct a lightweight load-owned Overlay object:
   allocate its collision-free runtime handle; attempt the Object registry, pointer-expiration,
   all-Abstract, and Tag-removal registry joins in that order; assign the preincremented native ID;
   then join the Overlay registry. Rust hard-fails on a registry-growth or later queue-growth failure
   instead of silently accepting native partial registration/dead-unqueued degradation. Direct-call
   base `ObjectClass::Unlimbo`, which virtual-dispatches base-then-derived `Mark(1)`. A
   malformed out-of-range type may become a safe typed Rust load error, but it is not documented as
   a native rejection filter. Each coordinate completes synchronously before the next: ordinary
   overlays run ordinary Mark, the four high anchors `0x18/0x19/0xED/0xEE` run their high structural stamp, and
   the eight low procedural triggers run the exact low Mark transaction on the same Scenario cursor
   continuation after prefix and Fill. Later packed coordinates can observe or overwrite earlier
   procedural writes; there is no component post-pass and no high-before-low phase split. Each
   allocated Overlay object dirties tactical state once through base
   `ObjectClass::Mark` before derived dispatch, including a later slope rejection. Its exact redraw
   helper argument is `0`, so it does not take the optional bridge-counter increment. Generated
   fixed/body cells do not repeat that object-level dirty. A row reaching the common tail completes
   all cell writes, optional ordinary CellAnim construction, and Recalc/eligible terrain-Anim
   construction before the next Overlay ID; then it sets `IsOnMap=0`, `InLimbo=1` and enters UnInit.
   UnInit broadcasts pointer expiration #1 while all five registry memberships remain, virtual-calls
   Limbo (a no-op because Mark already set `InLimbo`), clears alive, and appends the dead object to the
   shared deferred queue. It never reaches Display or Logic. A slope-admitted authored wall cannot
   predicate-fail here because successful Full_Init keeps ScenarioInit nonzero. It first completes the
   wall stamp/cleanup/connectivity/count effects and then takes this same common UnInit/dead/queue
   tail. The full-Limbo extra-broadcast wall rejection is retained only as a separate generic counter-
   zero caller path. A steep-slope `>4` non-`0xB2` row returns before cell
   writes/Recalc/UnInit after base Mark: it remains alive, `InLimbo=1`, `IsOnMap=1`, redraw-dirty,
   registered, ID-bearing, and unqueued until later scene teardown. Keep that lightweight survivor
   out of cell lists, `GameEntity`, occupancy, Display, Logic, current-object checksum, native save,
   `OverlayGrid`, and rendering; its registry membership and nonrefunded ID remain real lifecycle
   state. The constructor Terrain-blocker arm has the same alive/limbo/unqueued shape but is excluded
   from ordinary fresh authored reachability because `[Terrain]` loads later.
5. After the identity body finishes or is absent/empty, execute the independent positive-length
   OverlayDataPack body inside the same format gate and overwrite every allocated/in-radar real
   cell's state byte, even when identity was empty or rejected. The four high owners save and restore
   only their anchor state byte: their setter
   structural/neighbor writes persist, their common Recalc sees temporary `0/9`, then the owner
   restores the saved anchor byte. Ordinary non-high rows write identity and state `0`. A Land-code-5
   row then writes state `1` and synchronously executes the map-owned
   `SpreadCellGerminate(0)` algorithm. A Land-5 identity without `Tiberium=yes` returns immediately,
   performs no neighbor lookup, and retains the caller-written `1`; a flagged custom identity outside
   every configured image range maps to class `0` (its native diagnostic is non-sim observability).
   Otherwise make exactly eight ordered `N,NE,E,SE,S,SW,W,NW` neighbor lookups through the same fixed
   real-or-persistent-dummy seam; count neighbors with that same derived class (not exact overlay id,
   and without reading neighbor state); and rewrite only the receiver state from
   density table `[0,1,3,4,6,7,8,10,11]` for counts `0..8`. The zero argument consumes no RNG and the
   ignored return creates no Recalc, dirty, queue, bitmap, or heap mutation. True misses stamp and
   re-read the persistent dummy, so one dummy tiberium identity may count repeatedly. A crate row
   writes state `0xFF` last. Later OverlayData overwrites germinated density when present; without a
   data body, that source-order density survives into the later tiberium-queue rebuild. Every
   procedural write still recalculates immediately. After both bodies and temporary pixel-buffer
   cleanup, invoke the shared live deferred-finalization drain exactly once outside the format/body
   gate. It skips alive entries without stopping later dead entries; for each selected dead pointer it
   stable-erases every duplicate, invokes Release, processes the shifted successor at the same index,
   and rechecks live count so newly appended entries can be visited. Scalar finalization broadcasts
   pointer expiration again while memberships still exist (#2 for common/authored-wall success, #3
   only for generic counter-zero wall rejection), removes the Overlay registry, calls game-active
   base Limbo (already-limbo no-op), clears
   the type pointer, then base destruction removes deferred queue, Object registry, pointer-expiration,
   all-Abstract, and Tag listener memberships in that order before freeing the allocation; IDs are
   never refunded. Successful Overlay rows
   therefore stay dead/queued/registered through all later identity rows and the entire data body,
   then disappear before the first whole-map sweep; slope survivors remain. Generated/default-format
   reading constructs no authored Overlay objects but still runs this shared, non-Overlay-only drain
   over any preexisting queued-dead objects.
6. After the reader returns, always run the first whole-real-map `RecalcAttributes(-1)` sweep,
   independent of `NewINIFormat` and pack presence. It visits exactly `H*(2W-1)` cells from `(1,W)`
   in native anti-diagonal order and repeats live-neighbor LAT/slope, retail CliffBack, zone/cache,
   identity validation, and conditional terrain-animation latching. OverlayData is the final state-
   byte write, but Recalc does not read that byte; it preserves it unless identity validation clears
   identity and state. Capture the post-validation real identity/state vector plus the ordered
   authored real-cell blocker-neighbor plane in a separate non-Clone, consumed-once map-native
   `FinalizedOverlayPayload`; it must not be a field duplicated by
   `ResolvedTerrainGrid::clone`. An eligible authored cell may already have constructed its first
   terrain-attached AnimClass synchronously during an earlier per-Mark Recalc in decoded source
   order; its cell latch suppresses a duplicate, and this sweep constructs the remaining eligible
   cells in anti-diagonal order before authored Terrain/Technos. Each construction performs base
   Object registration, assigns a fresh native numeric ID independently of its collision-free runtime
   handle, initializes sound handles, and enters the live
   Anim registry before any valid `RandomRate` draw from the Scenario RNG. It then performs
   Reveal/Unlimbo and Logic/live registration without entity/cell occupation, and delay zero runs
   `Middle` synchronously. `Middle` may start the configured StartSound; it calls `Start` only when
   raw SHP frame-count/2 at AnimType `+0x298` is zero. Only after constructor/Middle returns does the
   producer write its terrain marker, Z adjustment, deletion marker, and cell latch. The Main RNG is
   absent from this corridor; all 20 active stock TileAnim rows also have zero RandomRate, while
   valid custom rows may draw. These transient identity, registry, Logic, RNG, and sound effects are
   not descriptors that may be postponed. If an otherwise eligible tile references a missing
   AnimType or its Anim allocation/registration cannot complete, return an explicit hard load error
   (or invariant panic in an infallible internal fixture); silently omitting the generation is not an
   accepted approximation of native's null-relative crash/degraded path. Keep Land, zone, LAT, compact caches, and bridge facts
   only in `ResolvedTerrainGrid` as derived projection.
7. Move that payload into `sim::overlay_grid::OverlayGrid::from_finalized_map_payload` one-for-one
   before native-equivalent Terrain/object mutation needs live overlay authority. Moving the payload
   invalidates the only production receipt; neither app nor sim retains a second copy. The constructor
   accepts no OverlayPack, OverlayDataPack, registry, Scenario, Mark, filter, or Recalc interface and
   performs no second decode. Terrain construction clears its source-cell tiberium and commits
   occupation to the same live terrain/overlay authority before the first Techno; later object
   sections must not be preprojected into the first sweep. Immediately after `[Terrain]` and before
   Unit, Aircraft, Infantry, Structure, or Smudge loading, initialize the growth queues and then the
   spread queues from the then-current live real-cell identity/state through a temporary read-only
   view owned by the sole sim ore-queue owner. This authored queue boundary observes post-Terrain
   resource clears but no later object occupancy. Retain that exact queue state through all later
   object-section loads and the final argument-0 cell pass; neither boundary may rebuild it. After
   Unit, Aircraft, Infantry, Structure, and Smudge loading, scan the live Anim registry in current
   order and remove every
   terrain-marked animation immediately through the native scalar-deleting-destructor path—not the
   ordinary Destroy/UnInit/pending-delete path. Each removal compacts the registry synchronously,
   releases and detaches its current sound handles without playing configured StopSound or creating
   ExpireAnim, and performs conditional owner cleanup; Recalc-created terrain animations are
   owner-null, so that branch makes no owner mutation. Do not invent entity/cell occupation for
   them. After all marked animations are gone, `InitCellAttributes` first visits every real cell and
   clears raw `Cell+0x140` bits `0x100000|0x200000`. Its next equal-count, equal-order anti-diagonal
   sweep zeros the opaque persisted/swizzled pointer slot at `Cell+0x30`, crosses
   `FUN_00483E30(0,0x10000,0,1000,1000,1000)`'s cell-light recomputation slot, and clears latch
   `0x20000`. The literals are only defaults: every ordinary cell recomputes current Scenario profile,
   admitted point-light, height, normalized RGB-key, and three brightness outputs through
   `FUN_00484180`; only sentinel ids `(0,0)` and `(-1,-1)` keep the neutral bundle. An AttachedTag whose
   event chain contains kind `0x19` then stamps raw `0x100000` across the complete rectangular map-
   bounds row through shared-dummy lookup; otherwise kind `0x1A` stamps raw `0x200000` across the
   complete bounds column. The `0x19` arm has precedence when both exist. Sparse misses accumulate
   both bits on the one dummy, which the first real-cell clear pass does not clear. The active reader is
   `FootClass::PerCellProcess @ 0x004D85D0`: entry into a marked cell offers every matching tag in
   row/column order. If its horizontal scan runs, the vertical gate reads the final row lookup—possibly
   the shared dummy—rather than the mover cell, while an admitted vertical scan still uses the mover's
   original X. Official `all01umd.map` contains reachable event `0x1A`. The stale
   `BridgeZone_NS/EW` labels are wrong, and these bits must not enter bridge topology, pathing, or
   `BridgeFacts`. Their exact generic-trigger behavior is an evidence-backed non-bridge exclusion.
   These three ancillary writes define ordered integration
   slots, not three new transaction-3 state owners. Transaction 3 must expose their native positions in
   its finalization trace/seam, invalidate any finalized/load-preview light cache exactly once at the
   cell-light recomputation-routing slot, and prove that none can enter `BridgeFacts`, topology, zones,
   or a newly invented cell field.
   The generic trigger subsystem owns the actual raw tag-line clear/restamp and `FootClass`-equivalent
   consumer behavior; transaction 20 owns semantic LightConvert/ZAdjust output and its cache test; and
   transaction 21/OQ-19 owns any decision to retain/restore the opaque `+0x30` pointer slot. Until those owners
   land, transaction 3 claims only exact sequencing/invalidation and negative non-ownership, not
   semantic parity for those foreign systems.

   The same main sweep then performs the argument-specific value/germination operation and the cell's
   Recalc. If the resulting current overlay is a wall, native reconstructs its owner from the nearest
   eligible Building **after that Recalc**. Rust already has the semantic wall-owner reconstruction;
   transaction 3 must preserve its final-current-identity ordering and reuse the existing owner rather
   than introducing a second algorithm. One global owner pass after all final-current Recalcs is
   output-equivalent because the helper reads no other cell's Recalc result. The Recalc creates the
   surviving animation set with new native IDs and the complete constructor/Middle/sound effects,
   preserves unrelated live-Anim order, and refreshes object-derived attributes.
   Terrain/object overlay clears and any second-sweep identity clear must use the ordinary
   synchronized OverlayGrid/ResolvedTerrain owners; they do not recapture or reconstruct raw packs.
   Publish app/presentation authority only from that post-object sim-owned state: the terrain template,
   occupied overlay render index, atlas/name dependency closure, minimap/radar inputs, and bridge
   presentation must include procedural identities absent from `MapFile::overlays` and exclude every
   identity cleared by admission or Recalc. Registry-level preloading may retain all runtime low
   variants, but raw pack membership is never final occupancy authority.
8. On generated `.SED`, synthetic Full_Init still executes its ungated Recalc and
   InitCellAttributes boundaries on the actual staged pre-materialization map. Omitted
   `NewINIFormat` defaults to `0`, so only the two encoded pack bodies are inert; an empty animation
   generation must not be inferred from that format gate. Capture/preserve any eligible synthetic
   Full_Init generation or prove zero for the exact staged state. The later generator directly
   materializes its complete three-wide overlay/data rectangle and must retain its own native phase
   boundaries: water/region/river/bridge work including CABHUT attempts; first whole-map Recalc at
   `0x00598E48` and its eligible animations; start-point work and AddTechBuildings/Neutral-Techno
   constructors; Tiberium and Recalcs at `0x00598FE7` and `0x00599153`; hills/LAT/trees/rocks with
   the direct LAT-helper Recalc at `0x005A4259`; final generator Recalc at `0x0059937D`; then
   initialize the tiberium growth/spread queues from that then-current state and free generator
   scratch; then call `InitCellAttributes(1)`. At the queue boundary the sole sim ore-queue owner
   reads the current map state through a temporary read-only view; it does not retain a second
   overlay grid or decode the packs. The final payload must not trigger another queue rebuild.
    `InitCellAttributes(1)` performs the same live-order immediate scalar deletion and crosses the same
    routed ancillary clear/opaque-zero/light/tag-line slots around the transaction-3-owned latch,
    post-value Recalc, and post-Recalc wall-owner ordering described above; at the argument-specific
    value-operation slot it calls the exact
    `SpreadCellGerminate(0)` helper from step 5. An absent or unrecognized resource identity contributes signed `0`; a recognized resource
   rewrites density and returns signed 32-bit `(state + 1) * TiberiumClass.Value`. Add each return to
   the native signed 32-bit local total with wrapping machine arithmetic before that cell's
   Recalc/recreation. No persistent owner or consumer of that total is proved, and this call does not
   rebuild the already initialized queues. Authored Full_Init's `InitCellAttributes(0)` is not
    animation-only either: inside that same common finalization sequence, it calls the value-only
   `Get_Tiberium_Value` for every real cell, contributes zero for non-resource cells or signed 32-bit
   `(existing_state + 1) * TiberiumClass.Value` for recognized resource cells to the same kind of
   wrapping total, and then Recalcs. `ScenarioClass::Full_Init` stores that argument-0 return in the
   persistent MapClass field at `+0x134` (`0x0087F91C`); cell-array teardown resets the field to zero.
   No active read of that field is proved, so retain the exact stored/reset state without inventing a
   gameplay, save, hash, or presentation consumer. It performs no germination and leaves the pre-
   object queue snapshot intact. This authored persistent write is deliberately asymmetric with the
   generated argument-1 call, whose return remains caller-local/ignored with no proved persistent owner.
   Extend `RmgConstructionTrace` or replace it with an equivalent consumed-once staged transport so
   CABHUT construction effects precede first-generator animation IDs/draws/sounds, which in turn
   precede Neutral-Techno effects, and later paint generations remain observable. Replay every
   actual Building constructor as Techno Scenario word, Building native unique ID, then placement outcome:
   every emitted or discarded Neutral-Tech construction consumes both but only emitted construction
   binds an entity; failed CABHUT site searches occur before construction and consume neither, while
   constructed stock CABHUTs consume both and emit. Preserve PR #170's bound constructor RNG words.
   Final cells cannot reconstruct this history. Preserve directly materialized identity/state
   through the same live owner and capture its final post-germination state in the same payload shape.
   Skip authored high/low Mark entirely: generated-native resource germination/Recalcs are mandatory,
   but the authored pack transaction and its post-pack replay are not. Explicit successful
   `.SED`/generated provenance, not overlay ids or construction-trace presence, selects this arm.
9. Rebuild records, zones and hierarchy only at the verified load boundary established by units 1,
   3, 4 and 5.

The app layer owns one explicit `FreshScenarioLoadContextDescriptor`, orthogonal to physical map
source. `LoadingRequest` creates it only for a fresh scenario from proven startup/session/replay
provenance, and `MapLoadInitial` carries it through that fresh-map load. Its normalized prefix kind
is one of stock offline, campaign, LAN, WOL-state-2, or replay-with-recorded-family; it contains only
  the roster/mode/House/start inputs the simulation prefix needs plus the closed OQ-34 consumed-once
`FreshFullInitNativeIdPrefixReceipt`/equivalent ordered constructor receipt. Stream restore is
deliberately not a variant. Network transport, replay I/O and UI session types remain in `app`;
`sim::scenario_bootstrap` receives the normalized fresh kind and inputs, not app or networking
dependencies. A seedless/generic current Rust entry may not guess stock offline. Every fresh
gameplay-equivalent Full_Init path requires the typed family and native-ID prefix receipt even when
signed `new_ini_format.unwrap_or(0) <= 1`, because House/Type/Cell construction, wrapping map-read
reservation, successful Tube construction, ungated Recalc Anims, and later objects remain active.
The format gate controls only the two pack bodies and Mark draws. Missing provenance returns an
explicit unsupported-load-context result before any native-ID/draw effect. A named pure-map
diagnostic may omit the receipt only if it constructs no live Object/Anim, spends no native ID, and
is explicitly excluded from gameplay/load-parity certification.

Production, `build_headless_terrain_bootstrap`, and every selector-free/auxiliary resolved-terrain
constructor receive the same explicit `MapLoadAdmissionDescriptor`/equivalent containing physical
source, mandatory typed fresh family/native-ID receipt for every fresh gameplay-equivalent path, and
signed `NewINIFormat` value. No auxiliary constructor may
default to Authored, infer Generated from trace presence, or guess stock offline. All routes invoke
one admission function and therefore return the same unsupported-context/missing-phase-transport
errors before any native-ID/Mark/draw, or the same finalized state, Scenario cursor, native-ID cursor,
and ordered identity trace when admitted. Convenience test builders may expose a named non-parity
pure-map fixture, but may not silently weaken the production contract or use it to certify an
authored format-inactive gameplay load.

The actual app source enum is `LoadedMapSource::{Loose, Mix, Generated, LegacyFallback}`. App loading
derives the map-layer `OverlayLoadSource` exactly once and carries it explicitly:

```text
Loose | Mix    -> OverlayLoadSource::Authored
Generated      -> OverlayLoadSource::GeneratedMaterialized
LegacyFallback -> unsupported for exact OverlayPack Mark until explicit provenance is supplied
```

This mapping is independent of `generated_construction_trace`: a `Generated` load with a missing or
empty trace still selects the no-authored-Mark arm, while Loose/Mix remain authored; it must then
return an explicit missing-phase-transport error rather than reconstruct lifecycle history or fall
back to authored Mark. Load context is a second, orthogonal gate. A stock-offline `Generated` path is accepted only for the proved chooser boundary,
Battle id `1` or FFA id `2`, with explicit accepted-preview start staging; arbitrary generated/mode
combinations and fresh external `.SED` injection are rejected. An authored campaign/LAN/WOL/replay
source can run Mark when `NewINIFormat > 1`; stream restore never runs Full_Init or Mark.

Persistence owns a separate `ScenarioRestoreContext`/equivalent no-Mark guard. It is created only by
the snapshot reader, never enters `LoadingRequest`, `MapLoadInitial`, the fresh prefix dispatcher or
the map pack routine, and applies the proved seed-zero Scenario poststate while transaction 21
reinstalls serialized authoritative overlay/runtime state and rebuilds only verified derived state.
There is no conversion from restore context to fresh-load descriptor. A regression fixture must fail
if restore invokes any Full_Init, Fill, OverlayPack Mark or fresh constructor-prefix entry.

`ScenarioBootstrapRng` is the sole pre-staging cursor owner and is consumed exactly once, before Fill,
into the staged Simulation load owner described above. From then on that Simulation is the sole
cursor/ID/registry/queue owner. During Fill and the later inline Mark transaction, app orchestration
lends a non-clonable raw-only adapter from that owner, but **no sim type crosses into `map`**. The app
invokes the map-owned inline OverlayPack routine with a map-native
`&mut dyn FnMut() -> u32`/equivalent raw-call interface backed by that borrow. `map` cannot range-wrap,
clone, reseed or import `sim`; it can only request the next raw word at the exact low-body write.
Inline processing applies `raw & 3` exactly `3*L` times on successful procedural body writes and
zero on every fixed/search/no-op/failure arm, then app releases the same borrow before authored
Techno construction. `src/map/overlay.rs` or the narrow low-Mark owner owns the recovered tables and
loop; `src/map/resolved_terrain.rs` owns the map-native finalized payload, overlay/state mutation and
Recalc projection, not Scenario. `sim::overlay_grid` may depend on and consume the map payload;
`map` never depends on `sim`.

The monolithic resolved-terrain build first splits at a pure pre-effect stage. After Fill materializes
the live cells and before OverlayPack/first Recalc, a map-owned eligibility/root-discovery surface
returns the scheduler Anim roots required by every reachable tile-animation constructor. App binds/
preloads that asset closure in production and headless paths. Discovery may inspect map, theater,
rules, and Fill-stage cells, but it cannot allocate a native ID or runtime handle, draw RNG, register
an Anim, start sound, set a latch, mutate OverlayPack state, or stand in for construction. It replaces
the current dependency on already-precomputed `tile_animations()` descriptors. Missing required
assets may hard-error before the first OverlayPack/Recalc Anim-construction effect; the already-spent
prefix native IDs and Fill RNG state are retained, not rolled back. Safe preload is not
live-construction authority.

The same already-existing staged Simulation then bridges map-owned Recalc timing to sim-owned
animation effects without reversing that dependency. `map` defines a narrow synchronous
`MapLoadEffectSink`/equivalent closure contract; the staged sim loader implements it with one
collision-free runtime-handle allocator, the independent native-ID counter initialized by the common
fresh-map prefix, Anim/Logic registries, sound owner, and Scenario borrow. Each per-Mark/full-sweep
construction or scalar-deletion effect is handled before the map routine returns to the next native
row/cell, so this is not a buffered descriptor/event vector. Map retains terrain eligibility, the
cell latch, and the sequencing point for post-constructor writes; sim retains live Anim identity,
registration, producer marker/Z/deletion fields, RNG, Middle, sound handles, owner-null state, and
immediate destructor semantics. The first generation remains live while the orchestrator constructs
Terrain. At the exact authored boundary after Terrain and before
Unit/Aircraft/Infantry/Structure/Smudge, the sole sim ore-queue owner initializes growth and then
spread queues from a temporary borrowed view of the current live map. The orchestrator retains those
queues unchanged while it constructs the later object sections and uses the same sink for the final
delete/unlatch/value-only/Recalc generation. The authored return is written to the map-load state's
exact `MapClass+0x134` analogue and reset only at the proved cell-array teardown boundary; it performs
no second queue initialization after occupancy changes. Generated phase-aware transport is consumed
through this identical owner; its separate queue-boundary effect likewise lets the sole sim ore-queue
owner inspect a borrowed pre-final map view before final germination, and later payload installation
is forbidden from rebuilding either authored or generated queues. A cloneable final render
descriptor may be derived only afterward and cannot authorize construction, deletion, an ID/draw, or
replay. The separate non-Clone
`FinalizedOverlayPayload` remains the only identity/state/authored-blocker-count transfer into live
`OverlayGrid` and the global pathfinding count owner; the live Anim/native-ID/registry owner itself is
never transferred from a temporary finalizer. The payload is the ordered authored counter result, not
a license to scan final `Wall=yes` identities.

The closed OQ-34 prefix extends that same orchestrator seam backward without moving a sim type into `map`. The app/load
orchestrator creates the sole fresh-Full_Init `NativeUniqueIdCursor` (or neutral equivalent) at
Clear_Scene; House/Type owners advance it directly, while Cell resize, map-read wrapping `+0x2710`,
successful Tube/Overlay allocation, and Anim construction synchronously request/record their effects
through a map-defined callback/sink. `FULL_INIT_AND_PREVIEW_NATIVE_ID_PREFIX_REINVESTIGATION_GHIDRA_REPORT.md`
fixes the exhaustive source-ordinal event families, campaign/noncampaign formulas, shadowed theater
window, and empty pre-reader shared-queue contribution. Allocation rejection/failure emits no ID where
native emits none; allocated Tube parse failure is the proved spend-then-error exception. A post-hoc count,
separate map/sim counters, or final-state reconstruction cannot preserve constructor/failure order.
The preview worker may return an ordered branch-specific lifecycle journal for atomic shell-side
application, but it must include every proved manual-storage Cell/dummy, Type, House/Super, and custom-
theater prefix effect before generator objects, use the preview owner's separate reset cursor, and
preserve retained cross-class numeric IDs plus final growth/spread queues on the exact-match/Cancel path.

The staged Simulation load runtime also owns a narrow shared `LoadObjectLifecycle`/equivalent rather
than putting authored Overlay constructors into `OverlayGrid` or `GameEntity`. This is not a stack-
local finalizer: the same non-render/non-save/non-hash lifecycle registry remains attached to the
process-scenario/Simulation owner after load and retains steep-slope survivors until scene teardown.
Load completion cannot transfer, reconstruct, or discard it from final cells. It keys lightweight Object
records by collision-free handles, tracks the four ordered base registry/listener memberships, the
post-ID Overlay registry, alive/limbo/on-map/redraw flags, and one shared duplicate-permitting
deferred queue. Common-success Overlay records, including every slope-admitted authored wall, stay
registry-resolvable through the entire data body. Successful Full_Init keeps ScenarioInit nonzero
while Overlay Mark runs, so the wall predicate short-circuits true: the wall completes its stamp/
cleanup/connectivity/count effects, reaches the common anchor Recalc, then records common UnInit
broadcast #1 -> already-limbo no-op -> death/enqueue. The separate counter-zero generic wall
predicate-failure path retains broadcast #1 -> full Limbo/Destroy/Mark-remove broadcast #2 -> death/
enqueue, but authored finalization must not select it. Steep-slope survivors
stay alive/unqueued until scene teardown but never acquire cell, Display, Logic, current-checksum,
save, or render authority. The reader epilogue invokes a shared live drain on this owner even when no
Overlay body ran. Fresh Full_Init seeds it with the proved empty prefix `[]`; it remains a shared queue,
not an Overlay-only type. It uses the existing preserve-
alive/remove-all-selected-duplicates/Release/process-shifted-successor semantics. Scalar destruction
broadcasts pointer expiration while memberships still exist (#2 common/authored wall, #3 only for the
generic counter-zero reject), then removes the
Overlay registry -> game-active Limbo no-op -> type pointer -> deferred queue -> Object registry ->
pointer-expiration listener -> all-Abstract listener -> Tag listener -> allocation, and never refunds
IDs.
Injected registry/queue growth failure aborts loading explicitly; allocation-null retains native's
no-construction/no-ID row outcome.

Every true fixed-table miss returned by the native lookup aliases one persistent shared dummy across
the whole pass. Transaction 3 therefore extends the existing `SharedCellDummy` owner with the
signed-dword overlay identity (`-1` on construction/Resize) and byte state (`0` on construction/
Resize) only; Land, zone, LAT, and compact caches remain real-cell derived state. Native lookup first
sign-extends the packed i16 coordinates and resolves signed `y*512+x`, so a negative component can
alias a real slot; no per-axis clamp may precede that lookup. A true miss stamps the same dummy
coordinate and preserves its prior overlay/state until another writer or Resize. Occupied-body
writes, missing lookups and edge rows still consume their exact raw words and mutate that dummy in
longitudinal/j order, while dummy Recalc jumps directly to its epilogue and changes nothing. Full-
state tests bracket prefix, Fill, Mark and the first authored constructor so a hidden second cursor,
ranged helper, reordered batch or lost dummy alias cannot pass. Constructor/Resize also retains the
dummy's native tile sentinel `0xFFFF` and flat slope for real-cell edge-LAT neighbor reads; those are
stable lookup inputs, not dummy Recalc outputs or finalized-payload fields.

Authored `Wall=yes` Mark adds one further live plane that cannot be derived from final cells. After
the universal slope gate, ScenarioInit forces wall acceptance. Native stamps identity/state, visits
N/E/S/W/self cleanup in order, computes same-compact-ID cardinal connectivity while Buildings do not
yet exist, keeps owner `-1`, then increments the N/NE/E/SE/S/SW/W/NW `CellClass+0x122` bytes with
wrapping `u8` before the common anchor Recalc/UnInit tail. Signed fixed-map lookups can alias a real
slot even when an axis looks out of range; such aliases update the real counter. A true dummy count
write has no fresh-game consumer and is not exported. OverlayData changes only state, and a later low
body can overwrite the wall identity without reversing its increment. Transaction 3 therefore retains
the real-cell authored count plane through `FinalizedOverlayPayload`, then composes runtime terrain,
building, foot, and wall lifecycle updates on the same global owner. Scanning final wall identities or
rectangular-clipping the authored neighbor pass is forbidden.

#### Path-planning transaction

1. Query entry using current/destination effective layers and structural facts.
2. Run A* with dual-layer visits, hierarchy, peer markers and direction-8 tube exits.
3. After success, run both native smoothing/straight-segment passes while carrying effective height and raw structural state.
4. Produce the final layered path before any entity relayer mutation.

#### Movement relayer transaction

1. Remove occupation/list membership using the old persistent plane.
2. Update location, height and `OnBridge` in the owning locomotor path.
3. Add occupation/list membership using the new plane.
4. Preserve explicit tube direction-8 progress/finalization separately.

Selected-layer scatter/crusher dispatch and stopped-blocked safety are distinct occupant-reaction transactions triggered from their verified callers; they are not appended to every successful relayer step.

#### Damage and collapse transaction

1. Preserve the four verified dispatcher paths, original impact-cell admission, strict Z window, native high/low family selection and RNG ownership.
2. Preserve deck/ground occupant-plane selection as an explicit input to ordinary splash, CellSpread tolerance and Rocker.
3. Let the unit-11 and unit-12 implementation contracts establish the still-open internal ordering among tolerance, ordinary damage, Rocker admission/dispatch and bridge state-machine admission; this design does not guess that order.
4. Once a bridge destroy primitive is entered, execute its BlowUpBridge fallout synchronously before the next destroyed cell.
5. Apply exact edge re-stamp, zones, events, radar/audio intents and DBRIS Anim/Bounce work in the contract-proved order.

##### Active wall-mutation host repair (transaction 3 critic loop)

Commit `95f77159` correctly moved wall identity, connectivity, Recalc, zone repair, retained
neighbor counts, fixed-grid aliases, and the shared dummy into one recursive overlay transaction.
It did not close the mechanism. Its `WallDamageResult::radar_dirty_cells` is an after-return
diagnostic ledger, while active production callers replay that ledger only after native Recalc and
zone work; it publishes no tactical dirty for damage. Runtime placement calls the same cleanup core
without a host, writes the owner before cleanup, and batches navigation after autofill. Those are
ordering defects, not presentation-only bookkeeping differences.

The correction generalizes the existing `WallDamageTransactionHost` into one narrow synchronous
wall-mutation host. The host receives ordered `Tactical` and `Radar` dirty steps using the packed
coordinate captured at the native call site, the already modeled navigation step, and pointer expiry
for either a represented real cell or the persistent shared dummy. It owns no overlay decisions and
draws no RNG. `OverlayGrid` remains the identity/connectivity/count owner; the world adapters remain
the tactical, radar, navigation, and entity-reference owners. The returned radar list may remain as
a diagnostic trace, but no production caller may use it as mutation authority.

For `DestroyOverlay`, an accepted hit emits tactical dirty before the `+0x10` state write. A retained
partial hit then returns with no radar step. A terminal hit completes the state write and any nested
cardinal transactions, clears the target, Recalcs it, publishes `AssignOrphaned`/graph repair, then
emits the target radar dirty. Its N/W/S/E cleanup receivers each visit N/E/S/W/self; every visit,
including non-walls and the shared dummy, captures one coordinate and emits tactical then radar before
the wall gate. A cleanup-cleared wall dispatches pointer expiry before Recalc, publishes Assign plus
graph only when its zone changed, and reverses eight retained counts only on that same changed-zone
branch. The direct target dispatches pointer expiry after the complete cleanup fan-out and then
reverses its eight counts. Signed fixed-grid aliases take the real-cell path. Shared-dummy dirty and
pointer callbacks remain ordered even when later probes restamp the dummy coordinate; production may
have no represented entity target to clear for that callback, but the native dispatch boundary is not
erased. The wider native pointer-listener roster remains an explicit residual outside the represented
cell-target references.

For one runtime `OverlayClass::Mark` wall cell, the transaction is indivisible and completes before
the next autofill cell: stamp data `0` and identity with no owner; run hosted cleanup in literal
N/E/S/W/self order; when not in editor/ScenarioInit, perform the anchor Merge plus graph step; then
write the owner; increment the wrapping eight-neighbor retained counts in N/NE/E/SE/S/SW/W/NW order;
then run the common anchor Recalc tail. Cleanup always Recalcs a visited wall, but publishes its own
zone repair/graph only when the zone changes. Authored identity finalization cannot itself seed a
damaged cleanup removal because Mark writes data `0` and OverlayData is applied later without another
cleanup. The removal branch is nevertheless active retail code/data-conditional for a pre-existing
isolated damaged GASAND, GAWALL, or NAWALL; no shipped-map witness or ordinary invariant-preserving
placement reachability is claimed. CYCL, BARB, and FENC stay dormant under retail rules.

The smallest borrow-safe production adaptation threads this host through the existing AoE prelude,
combat-inline hook, direct wall host, Lightning, sale, movement-crush, persistent-projectile, ambient,
and placement seams. A placement adapter borrows only the already separate entity-reference, dirty,
navigation, bridge, overlay, and terrain owners and commits each anchor/filler transaction before
continuing the scan. Load-time materialization retains its non-live wrapper and must not invent
runtime observer publication. Genetic Mutator remains excluded because the active retail
`[MutateExplosion]` path has no Wall/Wood/WallAbsoluteDestroyer capability.

Rejected alternatives are: passing raw world vectors through every overlay function, which couples
the map owner to Simulation storage and worsens borrow boundaries; returning a generic effect vector
for later replay, which recreates the exact deferred-order defect and loses nested transaction order;
and moving wall mutation wholesale onto `Simulation`, which duplicates OverlayGrid authority and
creates architecture drift. The callback host is the existing architecture seam extended only with
effects that native executes inside the same call.

Focused acceptance requires an exact ordered host spy for rejected, retained, terminal, nested,
cleanup-removal, signed-alias, and true-dummy cases; five-visit placement traces including non-walls;
owner visibility and second-anchor-Recalc ordering; one complete filler transaction before the next;
and production witnesses for ordinary combat, persistent projectile, Lightning, movement/crush,
sale, and the active-data-conditional placement cleanup. Tests must prove that no deferred production
radar replay remains. Existing exact connectivity/count/snapshot/hash tests remain preservation gates.

##### Transaction 3 residual ledger after PR #207 (critic 3, 2026-09-01)

Recorded open items, each with its native reading and owner. None is closed by PR #207.

- **OQ-37 post-`Full_Init` setup tail (`FUN_00684C30`) — IMPLEMENTED by transaction-3 slice B
  (`feature/bridge-post-load-tail`).** `ScenarioClass::Read_Scenario @ 0x00684620` calls
  `FUN_00684C30` after `Full_Init`. Live-read order: increment the editor/suppression counter; call
  `BuildingClass` vtable `+0x4E0` (`0x004456D0`) on every alive Building; `FUN_004F42F0(2)`
  (Tactical `+0xD7D` flag plus `MapClass::IncrementBridgeCounter`, presentation); non-editor
  `FUN_006D04F0(1)` sidebar toggle and, for `g_GameMode == 0` only, the campaign start-cell view
  setup; the TagType attach/registry pass; `[Basic] FillSilos` (`Scenario+0x34B2`) credits-to-
  tiberium loop (`HouseClass::Add_Tiberium_Credits` at `0x00684F45..0x00684F69`);
  `MapClass::ParanoidUnrevealAll(1,0)`; `FUN_0075F020` (sqrt/sin lookup tables, no sim state); a
  third full `RecalcAttributes(-1)` sweep (`0x00684FAB..0x00684FC0`); `ComputeBridgeZones @
  0x0056D6E0`; `RebuildZoneConnectivity @ 0x0056C510`; `RebuildAllZoneLevels @ 0x00581F50`;
  `FUN_00586BF0` (see transaction 4 below); then, when `DAT_00A8ED78` is null, one GasCloudSys
  `ParticleSystemClass @ 0x0062DC50` at leptons `(0xA80, 0xA80, 0)` whose constructor assigns a
  native ID (`ParticleSystemTypeClass::Find_Or_Allocate @ 0x00644630` finds the retail
  `[ParticleSystems]` entry, so no Type is constructed); then, when `Rules+0x1870` is non-null, an
  anti-diagonal `CellIterator` pass that draws `Random::RandomRanged @ 0x0065C7E0` on the Scenario
  RNG (`ECX = Scenario+0x218`, `0x00685095`) over `(0, Rules+0x186C - 1)` for every cell whose
  `Get_Tiberium_Value @ 0x00485020` is nonzero and constructs `AnimClass @ 0x00421EA0` at the
  `CellClass` vtable `+0x48` centre coordinate (`0x00486840`) with `(delay 0, loop 1, flags 0x600,
  ZAdjust 0, reverse 0)` on a zero roll; finally `FUN_0055AF40/50` write two globals with no
  simulation reader. `Clear_Scene @ 0x006851F0` deletes the particle system and nulls
  `DAT_00A8ED78` at `0x0068562E`, so every fresh `Full_Init` load reconstructs it (once-per-process
  behavior exists only between shell previews, which is G11's domain). Rules readers are verified:
  `ReadGeneral @ 0x0066D661..0x0066D699` reads `[General] OreTwinkle` (empty default, capacity
  0x80, `AnimTypeClass::Find_Or_Allocate @ 0x00428B80`) and `ReadAudioVisual @ 0x0066B7F8..
  0x0066B812` reads `[AudioVisual] OreTwinkleChance` (constructor default 0x32); retail sets
  `TWNK1`/30. The `AnimClass` constructor assigns its native ID and registers before the optional
  `RandomRate` draw, has no DetailLevel gate, and `TWNK1` (`LoopCount=-1`, `RandomLoopDelay=120,300`,
  `Rate=450`) draws nothing at construction. Slice B models the particle-system native-ID spend, the
  per-resource-cell Scenario draws in `CellIterator` order, and the zero-roll Anim construction with
  its native ID (`src/sim/ore_twinkle.rs`, `src/sim/scenario_post_map.rs`, rules
  `GeneralRules::{ore_twinkle, ore_twinkle_chance}`); the twinkle draws run at the end of
  `finalize_scenario_post_map`, after the modeled Post_Map_Init credit/crate/alliance work, for
  skirmish and generic fresh loads. The particle-system ID position differs by ingress: an authored
  load runs `FUN_00684C30` once (`Read_Scenario_INI @ 0x00686730` -> `Full_Init` with `XOR DL,DL`
  at `0x0068683A`), so the spend sits in the post-map pass; a generated launch first runs
  `RandomMapGenerator::InitMapFromSyntheticINI @ 0x00599650` (launch branch `0x00599A3A..
  0x00599A5B`: `Full_Init` with DL=1, whose `Clear_Scene` nulls `DAT_00A8ED78`, then `FUN_00684C30`),
  which constructs the object before any generator constructor, and the post-`Post_Map_Init` call
  finds it constructed and spends nothing. Rust mirrors both through
  `Simulation::construct_post_load_particle_system_id` (generated arm of `load_map_from_initial`
  before the construction-trace replay; post-map pass otherwise) and the
  `post_load_particle_system_constructed` flag. `HideIfNoOre` (`AnimType+0x359`) now has its
  `AnimClass::AI @ 0x00423AC0` consumer: before the MakeInfantry `vtable+0xF0` call, the
  bounce-landing block, and the trailer block, `AnimClass+0x19D` is rewritten every tick from the
  cell's `Get_Tiberium_Value`, so twinkles hide on harvested cells and reappear on regrowth
  (`Simulation::visit_anim`, draw-only suppression; the `+0x373` and `Rules+0xB8` AI blocks that also
  write `+0x19D` remain unmodeled, retail reachability UNCHECKED). Each live twinkle now also draws
  the Scenario RNG at every loop end (`RandomLoopDelay=120,300`, native `0x004247DA`, Rust
  `anim_class.rs` loop-end path) — consistent with gamemd, recorded because slice B activates that
  stream on every retail skirmish.
  `RandomRanged` bounds for `OreTwinkleChance <= 0` follow the native signed compare/swap
  (`SimRng::next_range_i32_inclusive`): chance 0 draws once over `{-1, 0}`, chance 1 draws nothing
  and spawns on every resource cell. Recorded, not modeled: the ParticleSystem object itself (no sim
  consumer proved), the FillSilos loop (scenario-flag-conditional, not modeled in Rust at all), the
  `+0x4E0` per-Building call (player-owned prebuilt Buildings only; kind switch on
  `BuildingType+0xEB8`, inert for a skirmish player at load), the tag-attach pass (trigger system),
  the campaign view setup, `MapClass::ParanoidUnrevealAll(1,0)` (re-shrouds load-time reveals
  before tick 0; owner: the shroud system; magnitude UNCHECKED), the constructor's `ZAdjust`
  substitution (`AnimClass @ 0x00421EA0` stores `AnimType+0x348` when the argument is 0; Rust's
  load-anim path stores 0; `AnimTypeClass @ 0x00427530` defaults `+0x348` to 0 and `[TWNK1]` sets
  no `ZAdjust`, so the two agree for retail), and a map-INI `OreTwinkle=` with an
  empty value (native keeps the rules pointer through the zero-length `ReadString` return; Rust reads
  the merged INI, so shadowing semantics are UNCHECKED; no retail map sets the key).
- **G10 generator tail: growth-then-spread queue order and final `InitCellAttributes(1)` germination
  — IMPLEMENTED by transaction-3 slice C (`feature/bridge-generated-germination`).** The generated arm
  of `load_map_from_initial` now runs, after `populate_staged_app_scenario`, the native queue
  initialization from the then-current painted densities (`TiberiumClass::InitGrowthQueues_All @
  0x00722D00` then `InitSpreadQueues_All @ 0x00722240`; `src/sim/runtime.rs::initialize_native_tiberium_queues`,
  shared with the authored Terrain/Techno seam) and then `MapClass::InitCellAttributes(1)` (`push 1` at
  `0x0059943F`, call at `0x0059944C`) as `src/sim/tiberium_germinate.rs::run_generated_final_cell_attributes`:
  every real cell in `CellIterator` order calls the shared `CellClass::SpreadCellGerminate @ 0x004818E0`
  (argument 0) model — eight `g_DirectionOffsets @ 0x0089F688` neighbours in N, NE, E, SE, S, SW, W, NW
  order through the stamping `MapClass::Get_CellClass @ 0x005657A0`, `g_OreDensityByNeighborCount @
  0x0081CD28[count % MaxDensity]`, return `(state + 1) * Value` summed into a caller-local wrapping total
  that is logged and never stored — and the post-map tail no longer rebuilds the queues
  (`tiberium_queues_preinitialized = true` for both arms). Generated presentation frames of resource
  identities are refreshed from the germinated grid. The crate Mark seam (`src/sim/crates.rs`) now routes
  through the same helper, which corrects its neighbour visit order from NW-first to the native N-first
  order (player-invisible; it changes only which missing neighbour the shared dummy's coordinate retains).
  Residuals kept open under G10: the per-cell Recalc / terrain-Anim scalar-delete-recreate chronology of
  that pass (the eager tile-anim set is the recreated set; `RecalcAttributes @ 0x0047D2B0` reads no
  `+0x11E`, so no attribute differs), the ancillary slots shared with the ancillary-seam bullet, the
  dummy's overlay identity at the `InitCellAttributes(1)` boundary (modelled as the post-Resize `-1`;
  generator writes through `Get_CellClass` misses are UNCHECKED), and the synthetic `Full_Init` phase
  journal. The crate Mark seam's own lookup (`resolve_crate_mark_cell`) still treats in-storage
  off-diamond cells as real where native NULL slots resolve to the dummy (DRIFT, diamond-edge crates
  only; fix by routing through `native_fixed_cell_index`, owner: the next crate slice). The slice also
  opened OQ-38 (native queue rebuild parity), which transaction-3 slice D then implemented (see the
  OQ-38 row and the bullet below).
- **OQ-38 native tiberium queue store parity — IMPLEMENTED by transaction-3 slice D
  (`feature/bridge-queue-rebuild-parity`).** The per-`TiberiumClass` stores the bridge load corridor
  seeds are now the native shape: `NativeTiberiumQueue` (append-only entry array with its counter, a
  1-based float min-heap of entry indices with `FloatMinHeap::SiftDown @ 0x005AD870` semantics and the
  `parent <= new` sift-up stop, capacity `(MapRect.Height + 4) * MapRect.Width * 2` from `FUN_0042B1F0`
  with the over-capacity array-only append), rebuilt per class in `CellIterator_Init/Next @
  0x00578350/0x00578290` order through `CanGrowTiberium @ 0x00483620` / `CanSpreadTiberium @ 0x00483690`
  (flat slope, `OverlayData < MaxDensity - 1` / `OverlayData > class index / 2`, percentage `>= 1e-05`
  as an x87 double compare, and the `CellClass+0xE4 FirstObject == 0` occupier gate modelled by
  `sim::tiberium::NativeCellObjectView`: the ground occupancy list joined with the live
  terrain-object cell index, since the `AddContent` wrapper `0x005683C0` links every terrain object
  through `TerrainClass::Mark @ 0x0071BFF8`, spawner or not; the spawner subset gates only
  `CanPlaceTiberium` targets). The processors (`GrowthProcessor @ 0x00722F00`, `SpreadProcessor @
  0x00722440`) pop the root, size the batch as `_ftol(FILD count * pct)` under the 53-bit chop
  control word `Math__ftol` installs on its first call (`native_x87`), draw `attempts = abs(raw) %
  batch + 1` in signed i32 (the word `0x80000000` pops one root and returns), gate on `pct > 1e-05`,
  reinsert on the literal `OverlayData < 0x0B` with priority `frame + abs(raw % 50)` whether or not
  the growth placement succeeded, spread the cell's CURRENT class from a stale entry, and rebuild
  their class when `count > capacity - 2 * attempts` / `count > capacity - 0x14`. `CanSpreadTiberium`
  takes no class, so `AddToSpreadQueue @ 0x00722AF0` admits a cell on its OWN class into whichever
  store the caller names: `Reduce_Tiberium @ 0x00480A80` queues every eligible neighbour of a fully
  removed cell into the REMOVED class's store (a harvested gem takes its ore neighbours, one
  `Random::Next` each), and only `RebuildSpreadQueue` tests `GetTiberiumType == class` first. The object view
  reaches the load-time rebuilds (authored, generated, post-map), the tick processors, the growth
  feed inside `PlaceTiberium`, the terrain-spawner placement, and the harvester, crater-smudge, and
  area-damage reduction reseeds (the `AoECellPrelude`/`commit_tiberium_reduction` traits carry the
  AoE transaction's occupancy). Snapshot restore rebuilds from the serialized
  `OreGrowthState::native_rect` (the `[Map] Size` rect; a state whose rect was never written falls
  back to the retained MapClass `Size`), never the cell-array extent. Rules keep the native percentage doubles
  (`growth_percentage_bits` / `spread_percentage_bits`). Critic chain: critic 1 NEEDS_FIX on
  `80e41172` (B1 restore rect, B2 terrain objects in `FirstObject`, R1/R2/N1/N3 edges), then critic 2
  NEEDS_FIX on `3142c09f` (B3: the full-removal reseed kept a Rust-only same-class test that native
  `CanSpreadTiberium` lacks, so a harvested gem beside ore skipped the native draw and flag write and
  diverged the Scenario stream; plus N1/N4/N5), all fixed with named regression tests in the
  contract's slice-D section, then critic 3 PASS on `51b51a3a` (both rounds rechecked against the
  native bodies; the `0x00722AF0` xref set proves `Reduce_Tiberium` is the only cross-class
  receiver), whose four follow-ups are applied. Deferred DRIFT, recorded in the
  OQ-38 row: the enqueue-side array-counter rebuild triggers (on the order of 260k frames on a 60x60
  map, hours of play but not unreachable), `MaxDensity` pinned to 12, native `atof` rounding of long
  decimals UNCHECKED. Open
  residuals, recorded: the PRE-EXISTING growth-driver timer multiplier DRIFT
  (`GrowthDriver_AllTypes @ 0x00722C40` reloads `Growth` scaled by `[0x007E5138]` = 0.3 when the
  Scenario flag byte's bit `0x40` is set and `[0x007E1718]` = 1.0 when clear; Rust always uses 1.0,
  so native growth can fire ~3.3x more often; bit `0x40`'s INI binding UNCHECKED, so the frequency
  cannot be named and the drivers are outside this slice's scope), the post-restore CDTimer reset (Rust runs both processors on the first tick
  after a restore; native `Load_Game_From_File`'s TiberiumClass timer handling UNCHECKED), the
  `AddContent` wrapper's fourth caller `AircraftClass::Mission_Hunt @ 0x00415565` and
  `VoxelAnimClass::AI`'s `AddToGrowthQueue` calls (neither claimed here), landed air-layer objects
  outside the ground-only object view, (d) the scenario-flag equivalence is UNCHECKED (native gates read
  `Scenario+0x34A6` / `SpecialFlags & 0x80`, bound to `[Basic] TiberiumGrowthEnabled` /
  `[SpecialFlags] TiberiumSpreads` only by
  `CELLCLASS_CANGROW_CANSPREAD_PREDICATES_GHIDRA_REPORT.md`, readers `0x00689E90` / `0x006B8CA0`
  not re-read; Rust additionally ANDs `[General] TiberiumSpreads`, native consumer uncited); the
  fallback post-map rebuild (no authored overlay registry) runs after Techno construction and so
  excludes ore under pre-placed units from the spread store where `Full_Init` seeds before
  Technos; the x87 chop premise holds after the process's first `_ftol`.
- **Value-only `Get_Tiberium_Value` aggregate / `MapClass+0x134` store (contract G6) — IMPLEMENTED
  by slice B.** The final authored sweep now calls the `Get_Tiberium_Value @ 0x00485020` model
  (`TiberiumClass.Value * (OverlayData + 1)`, signed wrapping) for every real cell before that cell's
  Recalc and stores the wrapping total in `Simulation::authored_tiberium_value_total`
  (`MapClass+0x134` / `0x0087F91C` analogue, `serde(skip)`, `None` on a new Simulation). A
  generated launch natively stores the synthetic `Full_Init`'s own `InitCellAttributes(0)` result
  on the pre-generation map (zero), which Rust leaves `None`; the generator's later argument-1 call
  is not stored. No active reader is proved and none is invented.
- **Ancillary `InitCellAttributes` slot seam.** The raw `0x300000` clear pass, per-cell `+0x30 = 0`,
  `FUN_00483E30(0,0x10000,0,1000,1000,1000)` light routing, latch clear, and AttachedTag `0x19`/`0x1A`
  restamp are not exposed as ordered slots by the final sweep. Owners remain the generic trigger
  subsystem, transaction 20, and transaction 21/OQ-19; transaction 3 continuation owes the ordered
  seam and the negative no-`BridgeFacts` assertion.
- **Retained wall plane `None` acceptance (contract G7) — IMPLEMENTED by transaction-3 slice E
  (`feature/bridge-transaction3-residuals`).** `OverlayGrid::from_native_overlay_packs` — the generated
  launch's overlay authority, and the only production caller of that boundary (an authored load
  reaches `from_finalized_map_payload` instead) — now owns the `CellClass+0x122` wall plane instead
  of leaving the legacy `None` compatibility mode: it starts at
  zero and wrapping-increments the eight neighbours of every accepted wall stamp in N, NE, E, SE, S,
  SW, W, NW order — `OverlayClass::Mark @ 0x005FC570`'s wall tail (`0x005FC758..0x005FC775`: eight
  `MapCoord_StepByDir_GetCell @ 0x00481810` steps over `g_DirectionOffsets`, each an 8-bit `INC DL`
  on `CellClass+0x122`, the anchor never incremented). Neighbour resolution goes through the
  allocation-aware lookup the runtime wall path uses, because `MapClass::Get_CellClass @ 0x005657A0`
  returns the shared dummy both for an out-of-array index and for a NULL pointer-table slot: a
  neighbour inside the storage rectangle but outside the allocated playfield takes no increment.
  Snapshot restore rejects a current-version state with no plane
  (`SnapshotRestoreError::MissingRetainedWallNeighborPlane`) rather than falling through to the
  identity rescan, so the blocker-count owner's legacy scan is now reachable only from test-only
  legacy constructors. The retail random-map generator emits only tiberium (`GEM01`-`GEM12` ids
  27-38, `TIB01`-`TIB12` ids 102-113), low-bridge deck (74-77 and 83-86 for the run cells, 92, 94,
  96 and 98 for the two ends of each axis) and `SROCK`/`TROCK` (168-177)
  indices; the `Wall=yes` ids in `rulesmd.ini` are `GASAND` 0, `GAWALL` 2, `NAWALL` 26, `CAFNCB` 203,
  `CAFNCW` 204, `CAKRMW` 240, `CAFNCP` 241 and `GAFWLL` 243 (registry ids in declaration order, not
  INI keys; `CYCL` has no section and so is not a wall), none of which the generator can emit — note
  `NAWALL` 26 sits one index below `GEM_BASE` 27, so an off-by-one there would start stamping walls.
  A generated launch therefore keeps an all-zero plane and nothing changes until a wall is built.
  From then on the count source is the Mark history, which nothing rebuilds from final wall
  identities — that is the authority hole the row closes, since a runtime-destroyed wall no longer
  silently drops its Mark history. Two native routines reverse a wall's contribution:
  `CellClass::DestroyOverlay @ 0x00480CB0` (`0x00481070..0x00481082`, unconditional once its removal
  path is taken) and `CellClass::PostDestructionWallCleanup @ 0x00480630`, whose own eight-step
  `DEC DL` loop (`0x00480999..0x004809EF`) runs only when that cell's hardcoded isolated removal
  fired (`TEST BL,BL` at `0x0048097D`) AND `RecalcAttributes` changed its zone type (the load at
  `0x00480972`, compare at `0x00480975`). Rust reproduces that gate at the two cleanup fan-out sites
  in `src/sim/overlay_grid.rs` and at the cross site in `src/sim/world/world_commands.rs`.
  A third route removes a wall and reverses nothing: `HouseClass::Sell_Building_At_Cell @
  0x004FCE80` clears the identity itself (`+0x44 = -1` at `0x004FCFBC`, `+0x11E = 0` at
  `0x004FCFCA`, `+0x50 = -1` at `0x004FCFDD`), then runs `RecalcAttributes(-1)` at `0x004FCFE7` and
  `PostDestructionWallCleanup(0)` at `0x004FCFFB` — whose per-cell gate rejects the just-cleared
  sold cell at `0x004807CA` — and touches `CellClass+0x122` nowhere in its body. A sold wall's eight
  increments stay in the byte permanently. Rust matches it on the plane accounting: the sale
  clears through `OverlayGrid::clear_overlay` with no plane decrement, and the cross skips the sold
  cell because it is no longer a wall. The claim is scoped to the player sale, which native reaches
  from `EventClass::Execute @ 0x004C6FCE`; the same routine is also called from
  `BuildingClass::Unlimbo @ 0x0044065C` and `@ 0x00440720` on the wall-over-wall replacement path,
  which is UNCHECKED against the Rust stamp transaction. That is the mechanism by which the plane, being a history counter, genuinely
  differs from what a final-identity rescan would produce, and it is ordinary play — selling a wall
  segment to reopen a lane permanently inflates the pathfinding blocker count on its eight
  neighbours, exactly as gamemd does. Before this row a generated launch rescanned identities, so
  the leak self-corrected there and did not match. PDWC's own zone-unchanged case is verified in
  code shape but its reachability is not demonstrated: `CellClass::RecalcZoneType @ 0x00483C80`
  stores zone `2` from the overlay Wall flag at `0x00483CD4` and returns, so a cleared wall cell's
  zone changes except through the building-occupancy re-entry at `0x00483E06`. At placement time the
  plane and a rescan agree except when a neighbour is unallocated, where the rescan's rectangular
  fan-out counts a cell the allocation-aware plane does not. The byte
  is a general blocker counter (`BuildingClass`, `FootClass` and `TerrainClass` limbo/unlimbo write
  it too, and `AStar_main_loop @ 0x00429EB1` is its sole reader); the retained plane is its wall-only
  slice, which Rust keeps as a separate contribution to the blocker counts.
  The per-miss shared-dummy coordinate restamp IS modelled: `MapClass::Get_CellClass` writes the
  requested packed coordinate to the dummy's `+0x24` on every miss (`0x005657C8`), and the
  allocation-aware lookup does the same, so after a wall stamp the dummy carries the last missed
  neighbour in `ADJACENT_8` order.
  Residuals recorded: (a) the map-pack boundary models only the `+0x122` tail of that wall branch —
  the `0x0047C620` acceptance gate whose failure aborts the whole Mark, the constructor-side
  `FUN_0047C550 @ 0x0047C550` gate that decides whether `ObjectClass::Unlimbo @ 0x005F4EC0` runs at
  all, `PostDestructionWallCleanup @ 0x00480630`, the `MergeAdjacentCellZone @ 0x0056D5A0` /
  `IncrementalRebuildZoneGraphAroundCell @ 0x00584550` pair and the `Cell+0x50` write are UNCHECKED
  at this boundary and unreachable while the generator emits no wall id; (b) the wrapping byte
  cannot overflow from wall Marks alone at the map-pack boundary (one pack entry per cell, so at
  most eight increments), but the `BuildingClass::Unlimbo` contribution to the same byte was not
  re-derived, so overflow overall is
  UNCHECKED: the map-pack pass takes at most eight increments per cell, but across a match the
  count is unbounded because the sale route above removes a wall without reversing it — exactly as
  native's is; (c) the only part of the cleanup gate still open is whether
  `CellClass::RecalcZoneType @ 0x00483C80` (which owns `Cell+0x4C`; `RecalcAttributes @ 0x0047D2B0`
  only calls it) and the Rust `recalc_zone_type` agree on the removal transition, so the two
  `Destroyed && zone_changed` predicates fire on the same cells. The overlay-priority half already
  has a gamemd-named check (`gsi_04_04_recalc_zone_type_overlay_priority_matches_gamemd`); (d) the
  destroyable-cliff collapse callback in `src/sim/world/mod.rs` clears every overlay identity in the
  replacement footprint through `clear_overlay`, which touches no plane, so a wall lost that way
  would keep its eight contributions. Which native routine performs that clear is UNCHECKED; the
  case needs a wall on cliff terrain, which is not wall-buildable, so it is not demonstrated to fire.
  The other production `clear_overlay` bypass, the wall sale, is verified native-faithful above and
  is NOT a residual; (e) the plane increments only for an entry the Rust acceptance filter
  accepts, and whether that filter is a superset, subset or neither of the native reader-side filter
  is UNCHECKED; (f) `SNAPSHOT_VERSION` stays 116, so a v116 state saved by an earlier build of this
  repo on a generated map is now rejected at load, and `Simulation::state_hash` folds the plane in
  (`retained-wall-neighbor-counts-v1`), so a generated launch's hash differs from `origin/main`'s
  from frame zero — intended, since the plane is authoritative state, and nothing was re-baselined
  because no committed hash golden reaches this constructor (the global harness builds its grid
  through `OverlayGrid::new`, and the bridge and slice6 harnesses install none at all, so the hash
  takes its no-grid early-out); (g) the Rust runs the eight increments before
  each stamp's `recalc_overlay_passability` where Mark runs its tail last — the counts are
  unaffected (they accumulate in a local plane and recalc never reads them), but the shared-dummy
  coordinate both paths stamp can end on a different value once a wall can reach this boundary.
- **CellAnim child fields.** `OverlayClass::Mark`'s ordinary tail constructs the CellAnim at
  `Location+0x180` per axis with `GetGroundHeight`, then, when the cell has a tiberium type, writes
  `Anim+0xD4 = ColorScheme[Tiberium+0xC0]+0x30C` and `Anim+0xFC = cell.nZAdjust_Ground`. The
  production host passes no remap and `z_adjust 0`, and its `+0x80` centre versus the native
  `+0x180` object-Location offset is UNCHECKED. Frequency zero on retail (every `CellAnim=` in
  `rulesmd.ini` is commented out); custom-content residual.
- **Startup-crate wall image uses the hostless placement wrapper** (`src/sim/crates.rs`, main-side
  phase14 code): `PostDestructionWallCleanup` publishes tactical and radar dirty per visit; the
  hostless wrapper does not. Frequency zero on retail (no `Wall=yes` crate image); recorded, not owned
  by the bridge program.
- **`FUN_00586BF0` (post-load bridge-record gap restamp) — routed to transaction 4/13.** After the
  zone rebuilds, `FUN_00684C30` walks the `MapClass+0x54` bridge-record array (count `+0x60`,
  0x10-byte records: start `(x,y)`, end `(x,y)`, byte `+8`, pointer `+0xC`). For records with
  `+0xC == 0` and byte `+8 == 0` it steps from start toward end along the record's axis and, on every
  cell lacking raw `0x100`, writes `0x400` with `0x800` cleared (NE-SW records) or ORs `0xC` into
  `Cell+0x141` (NW-SE records) across the five transverse cells `-2..=2` through the shared-dummy
  `GetCell` seam. This is destroyed-span restamping at load and belongs with BR-M10/BR-M17; it is not
  modeled by slice B.
- `MapClass::Resize @ 0x00567092/0x005670E2` -> `MapClass::InitZoneMap @ 0x00567110` ->
  `InitCellAttributes(0)` runs on the freshly constructed pre-Fill cells at every Resize: no Anim,
  ID, RNG, wall, or stored-total effect (`0x0087F91C` is written only by `Full_Init`'s own call at
  `0x00687B9C`). Recorded so the two authored Resizes are not mistaken for missing sweeps.
- Notes carried without action: `Land=9` is Railroad (retail `TRACKS01..16`) and the ordinary route
  yields identical bytes/Recalc/UnInit; `0xA7`/`0x7E` vein arms and `FUN_0074DE90` have zero retail
  content; germination's `% MaxDensity` is inert for `MaxDensity=12`; the transient
  `OverlayGrid`/terrain clone round-trip between the two finalize calls creates no divergent
  authority today; `FUN_00452D40` building wall extension after the second drain is UNCHECKED.

#### Repair and CABHUT transaction

1. Preserve the outer Y-major discriminator and inner X-major first-hit scan distinction.
2. Choose overlay, bridge-record or no-overlay fallback geometry exactly.
3. Apply repair mutations, pavement/flood-fill/level restoration only in their verified branches.
4. Rebuild zones/topology and emit observers/tags, sound/EVA and radar effects in native order.
5. Keep C4 timer, attached-bomb and hut-death entries distinct even when they share collapse machinery.

#### Restore transaction

1. Deserialize authoritative cell/object/tube/hut/effect state and the raw serialized RNG fields as
   separate inputs; do not assume every serialized RNG byte remains the post-load authority.
2. Apply the proved per-stream restore rule. Scenario's serialized `+0x218` bytes are read and then
   overwritten by native `Random::Seed(0)`, so the required poststate is the complete seed-zero
   Scenario state and the restore path runs no Mark. Main and MapGen restore behavior remains open
   under OQ-19 and transaction 21 until separately proved; neither may inherit Scenario's rule or an
   unchanged-continuation assumption.
3. Reconstruct raw/mutable bridge projections without retaining stale derived pointers.
4. Rebuild records, zones, hierarchy, radar dirties and render snapshots deterministically.
5. Validate unchanged object plane, tube progress and pending debris; validate Scenario's seed-zero
   poststate exactly, and validate Main/MapGen only against the later OQ-19 contract.

### 4. Keep content-conditional mechanisms installed but dormant without data

Explicit `[Tubes]`, bridge trigger actions/events and Psychic Sensor enemy action lines must be implemented in their normal owners. They consume no state and produce no behavior when the corresponding valid map/type content is absent. They must not be compiled out, replaced with stock-map prevalence assumptions, or merged into low Road behavior. Automatic same-cell TubeClass shells are a separate load-time mechanism: units 1 and 5 must classify their final-theater activation and preserve their zero-length, non-traversable boundary until native evidence proves an active consumer.

### 5. Carry evidence and closure state alongside work

Each mechanism progresses through:

```text
FROZEN-COVERAGE -> CONTRACTED -> BUILT+FOCUSED-VALIDATION
                -> CRITIC-N FINDING/FIX -> CRITIC-N+1 PASS -> CLOSED
```

Any new active caller reopens coverage. Any unresolved native term keeps the contract blocked. Any critic finding keeps the mechanism open. Passing a subset of tests never changes the owning GSI row to closed while another required mechanism is open.

### P0 — merged shared Techno-constructor RNG prerequisite and P0-R1 prefix correction

The original constructor P0, transaction 2, and P0-R1 are merged on current `main`; PR #196 closes
the smallest shared Scenario-prefix prerequisite needed to keep transaction 3's first Mark draw
exact. The completed all-context audit supplies transaction 3's campaign/LAN/WOL/replay/save/
generated/editor matrix; its typed preservation fixtures remain mandatory and cannot be waived by a
correct offline fixture. The
original constructor prerequisite is not a new bridge mechanism or a general Techno rewrite. It
models the one unconditional raw Scenario draw at
`TechnoClass__Constructor @ 0x006F3254`, stores the low word as
`GameEntity::techno_ctor_random_word`, and makes one internal construction funnel assign that field
exactly once.

The funnel receives an explicit initialization mode:

```rust
enum TechnoConstructorInit {
    FreshScenario,
    PreconsumedGenerated(GeneratedTechnoInit),
    Restored(u16),
}

struct StructureUpgradeLink {
    parent_stable_id: u64,
    slot: u8,
}
```

`FreshScenario` consumes and stores one raw word after valid type/house resolution and allocation
reach construction but before Unlimbo or any placement decision. It covers every active fresh
Techno ingress:

1. fixed authored `[Units]`, `[Aircraft]`, `[Infantry]`, and `[Structures]` in native section order,
   with increasing entry index within each section;
2. after a base structure successfully Unlimbos, each selected non-`-1` authored upgrade in native
   declared-count/slot order;
3. Post-Map starting MCV and extra-unit construction;
4. every ordinary runtime path that reaches `spawn_object_at_height` or
   `spawn_object_limbo_at_height`, including production, free units, sell survivors, slave-miner,
   spawn-manager, paradrop and other superweapon creation.

A later Unlimbo/placement failure never rolls the cursor back. Malformed rows, unknown house/type,
allocation failure, or another rejection before the native constructor consume zero.
`PreconsumedGenerated` validates the stable-index/type/cell binding from
`GeneratedTechnoInitTable`, installs its word and consumes zero. `Restored` reinstates the serialized
constructor word and consumes zero; this object-field preservation is independent of the separately
proved seed-zero Scenario RNG poststate.

Current `parse_map_entities` already preserves the four base section groups, but it must retain the
structure upgrade count and three type slots. After a base structure successfully Unlimbos, each
selected upgrade is constructed as a distinct `GameEntity`, owns its constructor word, and carries
`StructureUpgradeLink { parent_stable_id, slot }`. The link and word participate in snapshots and
hashes. This is the minimum faithful native constructor owner; it does not claim closure of the
broader structure-upgrade gameplay backlog.

Outside tests and deserialization, direct production use of `GameEntity::new_at_frame` must be
impossible or fail a source-boundary assertion. The common internal funnel owns the three current
construction sites in `world_spawn.rs`: map projection, `spawn_object_at_height`, and
`spawn_object_limbo_at_height`. `spawn_object` remains a delegating wrapper.

P0 gets its own implementation contract, focused `--lib` validation, commit, and fresh-critic loop.
Its required fixtures assert:

- exact fixed-map word binding and final cursor for one valid object in every section, a
  constructed-then-failed Unlimbo, a pre-construction rejection, and an upgrade entity with stable
  parent/slot identity;
- exact Post-Map starting-MCV and extra-unit word/cursor order, including a later placement failure;
- one ordinary placed runtime spawn and one limbo runtime spawn, each consuming and retaining its
  own word;
- a generated Techno projection with validated bindings consuming zero additional constructor draws;
- a snapshot round trip retaining every word and upgrade link without a constructor draw.

P0 remains open if any active fresh-construction ingress, section/entry order, upgrade event,
failure boundary, persistent owner, source mode, or cursor transfer is approximate.

## Implementation Transactions and Dependency Order

P0 plus the 21 bridge transactions below are dependency and native-transaction boundaries, not new
architecture layers and not mechanism pass gates.

| Order | Closure unit | Primary coverage | Dependency / ordinary-play oracle |
|---|---|---|---|
| 1 | Theater/rules/assets, raw flags, automatic-shell theater classification, TIBTRE mask preservation | BR-M01, BR-M06, BR-M24 | exact ten piece keys; raw-mask fixtures; automatic-shell corpus verdict; TIBTRE rejects `0x500`; retain raw SpecialFlags/session inputs for unit 10 |
| 2 | Active RMG preview/accept/`.SED` launch lifecycle, low deck/end/CABHUT production, and waterfall-topology exclusion | BR-M02, BR-M03 | P0 and unit 1; `7fee6929`, `4a63fa15`, `f1e6054b`, `a776f270`; fresh MapGen per run; first-entry/re-entry and no-preview gates; location-free discarded Neutral-Tech constructor events, with failed CABHUT pre-search absent; one launch `ScenarioBootstrapRng`; validated `GeneratedTechnoInitTable`; active-phase-only trace; complete stamped output; no generated Mark replay; `BuildRiverBridge` negative characterization |
| 3 | Shared authored OverlayPack/OverlayData and both load-time Recalc boundaries, fixed-map low procedural load and Road mutation | BR-M04 (shared high-load contribution only), BR-M05, BR-M11 | merged P0/P0-R1 and unit 1; complete active load-context cursor audit; exact y/x high/low/ordinary interleaving, pre-object payload handoff, authored per-Mark/first-sweep Anim construction and post-object scalar-delete/unlatch/recreation; Lost Lake/Killer plus destroyed low fixture; exact `NewINIFormat` activation; separate collision-free handles/native IDs with OQ-34's exact campaign/noncampaign fresh-Full_Init prefix, set-from-snapshot wrapping `+0x2710`, empty shared-queue prestate, and minimal Tube constructor-ID binding; final post-Recalc wall-owner reuse plus generic tag-line-bit exclusion and routed lighting/opaque-field side effects; generated-source no-Mark/direct-deck arm with actual synthetic-Full_Init state, phase-aware CABHUT/Neutral-Tech constructor and native-ID interleaving, every generator Recalc/Anim generation, final arg-1 unlatch/germination-value/Recalc lifecycle, and exact preview-native reset/replacement/Cancel/re-entry/queue lifetime |
| 4 | High topology, records, zones, hierarchy and edge restamp | BR-M04, BR-M10, BR-M17 | unit 1; Bay of Pigs/Hills and Deadman's Ridge |
| 5 | Explicit TubeClass load/hierarchy/direction-8/persistence and automatic-shell non-traversal | BR-M12, part of BR-M22 | units 1/4; sealed valid custom tube fixture; zero-length shell negative case |
| 6 | Dual occupancy, entry, A*, peer markers and locomotor transitions | BR-M07, BR-M09, BR-M13 | units 4/5; deck, under-span, ramp, gap |
| 7 | Two native post-A* smoothing passes | BR-M25 | unit 6; no wrong-plane shortcut |
| 8 | Selected-layer scatter/crusher and stuck safety | BR-M23 | unit 6; ten-object order and bridge exemption |
| 9 | Spawn, Unlimbo, landing, paradrop, teleport and relayers | BR-M08, part of BR-M13/BR-M21 | unit 6; correct list/height after placement |
| 10 | Impact admission, destruction-authority matrix, Z/family gates and four-path RNG | BR-M01, BR-M16 | units 1/3/4; campaign/editor SpecialFlags, complete skirmish and true-network session `BridgeDestruction` handoff, CombatDamage non-owner, CABHUT bypass, strict impact boundary and negative family |
| 11 | CellSpread bridge-object tolerance | BR-M27 | unit 10; V3WH/V3EWH/DMISLWH plane fixtures |
| 12 | AoE Rocker admission, enumeration, selected-plane dispatch and exact impulse/timer state | BR-M26 | units 10/11; native numeric attenuation/timer/order oracle plus Rocker-negative case |
| 13 | High/low ladders, setters and direct walkers | BR-M17 | units 3/4/10; exact mutation/state trace |
| 14 | Synchronous fallout and DBRIS Anim/Bounce | BR-M15, BR-M18 | units 12/13; object order, RNG, bounce damage |
| 15 | Engineer repair and no-overlay restoration | BR-M19 | units 3/4/13; scan order and terrain/height tail |
| 16 | CABHUT cursor, timer, attached bomb and fallback collapse | BR-M19 | units 13/15; each entry and negative gate |
| 17 | Repair observers/tags and multi-engineer order | BR-M18, BR-M19 | units 15/16; event/output ordering |
| 18 | Projectile, fire, superweapon and remaining effect-plane consumers | BR-M14, parts of BR-M15/BR-M16/BR-M21 | units 6/10; preserve Bullet crossing and close gates |
| 19 | Tactical inverse, acquisition, Mirage, placement, orders and triggers | BR-M21 | units 4/6/18; deck/under-span interaction fixtures |
| 20 | Render, split, shadow/railing, PixelFX, action lines, radar/audio | BR-M20 | all sim facts stable; frame/order and raw-bit fixtures |
| 21 | Save/load/checksum/rebuild and deterministic projection | BR-M22 | authoritative state/derived rebuild stable; per-stream restore oracle with proved Scenario seed-zero and OQ-19-gated Main/MapGen |

### Mechanism closure gates

Every dependency-coherent transaction has one builder and its own fresh read-only critic chain on a
short-lived transaction branch. Builder identity need not persist across nonadjacent transactions;
the durable owner is the cumulative evidence bundle for each `BR-M` row. A transaction critic checks
the full transaction requirement and every mechanism contribution it touches. When a transaction is
the last contributor to a split row, a fresh critic also receives that row's complete native/retail
requirement, all earlier contributing diffs and outputs, routed questions, exclusions, and current
preservation evidence. No earlier transaction or different row's critic result can close it.

| Mechanism gate | Contributing transaction(s) |
|---|---|
| BR-M01 | 1 and 10 |
| BR-M02 | 2 |
| BR-M03 | 2 (negative characterization) |
| BR-M04 | 3 (shared high OverlayPack/OverlayData/two-Recalc-boundary contribution) and 4 |
| BR-M05 | 3 |
| BR-M06 | 1 |
| BR-M07 | 6 |
| BR-M08 | 9 |
| BR-M09 | 6 |
| BR-M10 | 4 |
| BR-M11 | 3 and later mutation-preservation checks in 13/15 |
| BR-M12 | 5 |
| BR-M13 | 6 and 9 |
| BR-M14 | 18 |
| BR-M15 | 14 and 18 |
| BR-M16 | 10 and 18 |
| BR-M17 | 4 and 13 |
| BR-M18 | 14 and 17 |
| BR-M19 | 15, 16 and 17 |
| BR-M20 | 20 |
| BR-M21 | 9, 18 and 19 |
| BR-M22 | 5 and 21 |
| BR-M23 | 8 |
| BR-M24 | 1 |
| BR-M25 | 7 |
| BR-M26 | 12 |
| BR-M27 | 11 |

A row passes only after all of its contributing transactions, preservation tests, routed open
questions, and negative facts are exact and a critic who did not build it returns no material
finding. Split rows therefore remain open after an early transaction. Bundled transactions must be
decomposed into reviewable mechanism-scoped deltas or a separately named prerequisite commit within
that transaction; a shared commit never grants a shared pass.

Transaction 3 begins BR-M04's shared high-load contribution but cannot close BR-M04. Its
evidence/output bundle must nevertheless include the native interleaving,
OverlayData/two-Recalc-boundary order, finalized-payload handoff and high-anchor retail preservation
fixtures; transaction 3's fresh critic checks that contribution for escaped high-load regressions.
After transaction 3 merges, transaction 4 uses a new short-lived branch and may use a different
builder. Its fresh critic receives BR-M04's complete transactions-3-and-4 requirement, native
evidence, cumulative diff and validation output. Neither BR-M05's pass nor transaction 3's
preservation check substitutes for that full BR-M04 critic gate.

### Open-question routing

Every frozen question has a pre-implementation owner. A unit cannot become `CONTRACTED` until all questions routed to it are resolved or proved inapplicable by active-retail evidence.

| Open questions | Owning closure unit(s) |
|---|---|
| OQ-01 complete consumer census | every unit as it closes; final bridge-wide reverse audit is the zero-add owner |
| OQ-02 runtime constants | 6, 9, 14, 20 |
| OQ-03 automatic tubes; OQ-21 activity label | 1 for theater-corpus classification; 5 for shell identity/non-traversal |
| OQ-04 RMG low placer; OQ-22 waterfall ownership | 2 |
| OQ-05 low overlay Mark RNG | 3 |
| OQ-06 traversal call placement; OQ-07 A* ties/hierarchy; OQ-09 peer markers | 6 |
| OQ-08 tube hierarchy pairs | 5 |
| OQ-10 `ShouldBeOnBridge` reachability/event consumers | 6 and 19 |
| OQ-11 locomotor reachability | 6 and 9 |
| OQ-12 teleport/landing state | 9 |
| OQ-13 projectile/effect families | 14 and 18 |
| OQ-14 edge restamp | 4 and 13 |
| OQ-15 collapse debris | 14 |
| OQ-16 repair selection/restoration | 15, 16 and 17 |
| OQ-17 rendering assets | 1 for asset identity; 20 for presentation binding |
| OQ-18 cross-system consumers | 9, 18 and 19 |
| OQ-19 persistence | 5 and 21 |
| OQ-20 retail fixtures | each owning unit; final reverse audit owns the complete matrix |
| OQ-23 low repair tail | 15 |
| OQ-24 low bridge/TubeClass split | 3 and 5 |
| OQ-25 scatter/crusher/stuck dispatch | 8 |
| OQ-26 target acquisition layer gate | 19 |
| OQ-27 raw-mask terrain/resource consumers | 1 and final reverse audit |
| OQ-28 action-line endpoint height | 20 |
| OQ-29 post-A* smoothing | 7 |
| OQ-30 Rocker secondary effect | 12 |
| OQ-31 CellSpread tolerance | 11 |
| OQ-32 RMG preview unique-ID continuation and terminal Anim/sound cleanup | RESOLVED by `RMG_PREVIEW_ANIM_BUILDING_IDENTITY_LIFECYCLE_REINVESTIGATION_GHIDRA_REPORT.md`; transaction 3 implements and validates it |
| OQ-33 authored OverlayPack ephemeral OverlayClass ID/registry/deferred-drain lifecycle | RESOLVED by `AUTHORED_OVERLAY_EPHEMERAL_OBJECT_FINALIZATION_REINVESTIGATION_GHIDRA_REPORT.md`; transaction 3 implements and validates it |
| OQ-34 complete native-ID prefix before the first generator/authored object: preview Set_Defaults/manual-storage branches plus fresh-Full_Init pre-map House/Type/Cell constructors, map-read transform, and Tube prefix | RESOLVED by `FULL_INIT_AND_PREVIEW_NATIVE_ID_PREFIX_REINVESTIGATION_GHIDRA_REPORT.md`; transaction 3 implements the consumed-once prefix/preview lifetime, while positive Tube topology remains 5 |
| OQ-35 `InitCellAttributes` raw `0x100000`/`0x200000` clear/restamp identity and consumers | RESOLVED as active generic AttachedTag event-`0x19`/`0x1A` row/column trigger acceleration, not bridge-zone topology; transaction 3 exposes the ordered ancillary seam and negative no-`BridgeFacts` assertion but does not implement or close the official-retail-reachable generic bits/consumer |
| OQ-36 `InitCellAttributes` ordinary-cell LightConvert/ZAdjust recomputation | native ordinary/sentinel split and draw consumers are verified; transaction 3 executes and tests one cache invalidation at the recomputation-routing slot, while transaction 20 owns semantic rendered-cell-light equivalence and the final end-to-end stale-preview-cache test |
| OQ-37 post-`Full_Init` OreTwinkle Scenario-RNG pass and the `FUN_00684C30` post-load order (third Recalc sweep, bridge-zone/zone-connectivity/zone-level rebuilds, particle-system ID, twinkle draws) | IMPLEMENTED by transaction-3 slice B (particle-system ID, per-resource-cell Scenario draws, zero-roll Anim construction); `FUN_00586BF0` bridge-record restamp routed to transaction 4/13; FillSilos loop and the `+0x4E0` Building call recorded as non-bridge residuals |
| OQ-38 native tiberium queue store parity (`TiberiumClass::InitGrowthQueues_All @ 0x00722D00` / `InitSpreadQueues_All @ 0x00722240`, `RebuildGrowthQueue @ 0x007233A0` / `RebuildSpreadQueue @ 0x007228B0`, `GrowthProcessor @ 0x00722F00` / `SpreadProcessor @ 0x00722440`, `AddToGrowthQueue @ 0x007235A0` / `AddToSpreadQueue @ 0x00722AF0`, `CellClass::CanGrowTiberium @ 0x00483620` / `CanSpreadTiberium @ 0x00483690`, `FloatMinHeap::SiftDown @ 0x005AD870`, `Math__ftol @ 0x007C5F00`; decompiled live 2026-09-02) | IMPLEMENTED by transaction-3 slice D (`feature/bridge-queue-rebuild-parity`): `src/sim/ore_growth.rs::NativeTiberiumQueue` models the append-only entry array, the 1-based float min-heap with the native sift-up/sift-down, and the `(MapRect.Height + 4) * MapRect.Width * 2` capacity; rebuilds walk `CellIterator` order per class; the processors pop the heap root, size the batch with the x87 chop product and `_ftol`, gate on `pct > 1e-05`, reinsert on the literal `OverlayData < 0x0B`, and rebuild on `count > capacity - 2 * attempts` / `count > capacity - 0x14`; admission gates on `pct >= 1e-05` and the `CellClass+0xE4 FirstObject == 0` occupier test (`sim::tiberium::NativeCellObjectView`: ground occupancy joined with the live terrain-object cell index, threaded through the load rebuilds, the tick processors, the growth feed, the terrain-spawner placement, and the harvester/crater/area-damage reductions); snapshot restore rebuilds from the serialized `[Map] Size` rect. Critic 1 NEEDS_FIX on `80e41172` (B1 restore rect, B2 terrain objects) and critic 2 NEEDS_FIX on `3142c09f` (B3 cross-class `AddToSpreadQueue` admission after a full removal), fixed with named tests. Deferred DRIFT, recorded: the enqueue-side array-counter rebuild triggers (`counter > capacity - 10` growth, `counter >= capacity - 0x14` spread; on the order of 260k frames on a 60x60 map, hours of play but not unreachable), `MaxDensity` pinned to 12 (`TiberiumClass+0xE4` initializer not re-read), native `atof` rounding of long percentage decimals UNCHECKED. Open residuals: (d) scenario-flag equivalence UNCHECKED (`Scenario+0x34A6` / `SpecialFlags & 0x80` bindings from `CELLCLASS_CANGROW_CANSPREAD_PREDICATES_GHIDRA_REPORT.md`, readers `0x00689E90` / `0x006B8CA0` not re-read; Rust also ANDs `[General] TiberiumSpreads`); the fallback post-map rebuild (no authored overlay registry) seeds after Technos where `Full_Init` seeds before them; the x87 chop premise holds after the process's first `_ftol` |

Two ancillary writes do not need new question numbers. Final wall-owner reconstruction is already a
verified semantic match under GSI-04.07; transaction 3 must preserve its post-final-Recalc ordering
when refactoring finalization. `Cell+0x30=0` is a persisted/swizzled pointer-shaped lifecycle slot but
has no proved live gameplay producer/consumer in this corridor; its meaning stays under OQ-19/
transaction 21 and must not be invented as bridge state or numeric scratch.

The destruction-authority matrix has an additional mandatory deferred evidence term already present
in the frozen source set: the complete UI/session writer chain into `DAT_00A8B260` and the true
network-multiplayer handoff are not yet proved. Transaction 10 and the BR-M01/BR-M16 mechanism
contracts must resolve `0x006B8AE0`, `0x006B8CA0`, `0x00671EA0`, and `Full_Init` branches
`0x0068794D..0x00687966`/`0x00687C16..0x00687C29` against active retail modes. Until that proof or an
evidence-backed exclusion exists, neither BR-M01 nor BR-M16 may pass. This corrects the design's
earlier claim of a fully settled matrix; it does not silently add or waive a frozen open question.

The fourth-review prerequisite concerning preview/cancel/accept/`.SED` dual-RNG ownership and the
Revision-4 critic findings concerning one launch cursor, generated-object bindings and generated
overlay replay, plus Revision-5 findings concerning discarded-event shape, dormant helper exclusion
and authored constructor ownership, and Revision-6 findings concerning Post-Map/runtime construction,
constructor-word persistence and upgrade identity are not left as new open questions. They are
resolved before implementation by the exhaustive lifecycle report at `7fee6929`, `4a63fa15`,
`f1e6054b`, and `a776f270`. They are mandatory P0/unit-2/unit-3 contract inputs.

### Evidence-backed exclusion routing

Negative facts are closure requirements, not prose-only cautions. Each routed unit must include the applicable do-not-do test or source assertion in its implementation contract, and its critic must check that the excluded behavior was not introduced.

| Frozen negative fact(s) | Owning closure unit(s) |
|---|---|
| TS Mech, DropPod and subterranean bindings; inactive meteor and LaserFence branches | 5, 6, 9, 14 and 18 as applicable to the inherited lead |
| `TooBigToFitUnderBridge` is render-only, never movement/passability | 6 and 20 |
| automatic same-cell tubes are never joined or given direction-8 movement; low Road never receives high/tube state | 3, 5 and 6 |
| `BuildRiverBridge` waterfall shaping and unreferenced type-3/4 block are not runtime bridge topology | 2 |
| `TrainBridgeSet` is TS-only; `RAILBRDG` remains render data rather than train topology | 1, 4 and 20 |
| shore/water/RMG helper labels, WaterBridge TMP names, refinery radio `0x16`, Golden Gate scenery names and generic TagType registry names are not physical bridge mechanisms | 1, 2, 3, 17 and 19 according to the real owner |
| active YR has no proactive bridge-specific AI attack/repair/destroy opcode, CABHUT priority or special route policy; ordinary targeting/orders/pathing remain active | 6, 16 and 19 |
| no continuous per-cell bridge HP; Scenario/session SpecialFlags authority is not `[CombatDamage]`; weapon-damage authority does not gate CABHUT/attached-bomb collapse | 1, 10, 13 and 16 |
| CABHUT immunity, ownership/capture and `MultiEngineer` do not suppress verified collapse/repair transactions; hut placement does not repair automatically | 15, 16 and 17 |
| collapse force-damages ground occupants and DropIns deck occupants; it does not force-kill deck occupants; `BridgeVoxelMax` does not gate metallic debris | 14 |
| stale `UnregisterBridgeRepairHut` and `RecalcBridgeShroudFlags` names do not create hut-registry or per-layer-shroud mechanisms | 17, 19 and 20 |
| no dedicated bridge network packet; deterministic simulation/RNG carries results | 21 |
| map-editor fallout suppression is not an ordinary gameplay branch | 14 |
| malformed sentinel-less tube crashes may be rejected safely; editor-only tube/placement writing UI remains excluded while runtime placement stays active | 5 and 19 |
| no per-layer shroud model; cold BSS zero never proves a runtime constant dormant | every affected contract, with units 6, 14, 19 and 20 as the known consumers |
| accepted RMG preview data/MapGen continuation and its live shell-native objects are never gameplay authority; preview Scenario state is never carried through Start, but Cancel/re-entry must not prematurely destroy the proved shell Buildings/Anims/latches/sounds or counter; successful output objects cannot stand in for discarded Neutral-Tech constructor events, while failed CABHUT pre-search is not an event; generated low-deck cells never receive a fixed-map Mark replay | 2, 3 and 21 |
| no-xref RMG-shaped `0x005A6510`/`0x005A82E0`/`0x005A91E0` are not active generator phases; discarded neutral-Techno events do not invent a final cell | 2 |
| fixed authored, Post-Map and runtime Techno construction are not RNG-free; generated Techno-binding projection and snapshot restore are not allowed to draw a constructor word again, while staged generated Anim RandomRate remains native | P0, 2 and 3 |

## Builder, Critic, and Publication Protocol

For every dependency-coherent implementation transaction:

1. Refresh its living-inventory rows against then-current `main`. Resolve every material behavior and routed open question needed by the transaction against active `gamemd.exe` and retail data into one sourced transaction contract. OpenTS may locate functions but supplies no required behavior.
2. Create one short-lived `feature/<transaction>` branch from freshly fetched current `origin/main`. Assign one builder for the transaction. It may preserve correct code, replace wrong code, and implement missing code only within that transaction and its smallest verified prerequisite.
3. Check `cargo`/`rustc` ownership before validation. While building, run focused `cargo test -p vera20k --lib <filter>` commands only; never a bare Cargo test.
4. Commit each coherent evidence-backed slice after focused validation. Keep the branch reviewable and buildable; do not defer a multi-slice transaction into one giant commit.
5. Give a fresh read-only critic who did not build the transaction its complete requirement, native/retail evidence, exact cumulative diff, preservation obligations, and literal validation output. If the transaction is the last contributor to a split mechanism, include that mechanism's complete earlier transaction history and row-level closure bundle.
6. If it fails, the builder fixes the largest finding and commits the correction. Give the full updated bundle to a new fresh critic, who must recheck every prior finding plus the new diff. Repeat until a fresh critic passes with no material finding.
7. After the fresh pass, check Cargo ownership and run `cargo test -p vera20k --lib` exactly once as that PR's full readiness certification. Do not rerun it for the same PR unless a later code correction invalidates the certification; such a correction reopens readiness and requires the rule in `AGENTS.md` to decide the new certification boundary.
8. Push the dedicated branch and open or update its PR targeting `main`; publication is preauthorized by this goal. Record the contract, exact commits, critic chain, focused literal output, and full-suite literal output. Take the PR through review and merge before beginning any dependent transaction. Never stack a dependent branch or rewrite around this boundary.
9. Refresh the living inventory from the resulting `main`. A passed transaction closes only the exact contributions it contains. Approximate, unverified, missing, or residual behavior keeps its mechanism and owning GSI row open; a split mechanism closes only at its final cumulative row review.

Critics do not edit. Builders do not self-approve. A critic pass and PR certification prove only the
bounded transaction and any explicitly completed cumulative row gate, not the bridge system. The
final bridge-wide reverse audit consumes the per-PR certifications; it does not rerun a blanket full
suite unless it discovers a correction requiring a new transaction and PR.

## Player-Experience Detail Ledger

- `MILESTONE-BLOCKING` — ordinary units must select and remain on the correct deck/ground plane through entry, A*, smoothing, locomotion and occupancy. Trigger: every high-bridge crossing. Player effect: refused routes, wrong-layer shortcuts, overlap or units falling between layers. Frequency: common on high-bridge maps. [BR-M07, M09, M13, M23, M25]
- `MILESTONE-BLOCKING` — low bridges must remain flat Road overlays and mutate exactly through intact/damaged/destroyed/repair states. Trigger: every low crossing and bridge damage. Player effect: wrong movement class, impassable water or invented tunnel behavior. Frequency: common on stock low-bridge maps. [BR-M05, M11, M17, M19]
- `MILESTONE-BLOCKING` — collapse and repair must preserve native per-cell transaction/RNG order. Trigger: bridge weapon damage, CABHUT C4, attached bombs or engineer repair. Player effect: different survivors, debris damage, bridge shape, zones and events. Frequency: common whenever bridges are contested. [BR-M16..M19, M26, M27]
- `MILESTONE-BLOCKING` — bridge destruction authority must follow the active mode/source matrix: scenario `[SpecialFlags]` where authoritative, skirmish/multiplayer session `BridgeDestruction` where authoritative, never `[CombatDamage] DestroyableBridges`; CABHUT C4/attached bombs bypass the weapon gate. Trigger: every attempted weapon or hut-driven collapse when sources disagree. Player effect: bridges become wrongly indestructible/destructible or hut sabotage stops working. Frequency: every configured disagreement and every CABHUT collapse. [BR-M01, BR-M16, BR-M19]
- `MILESTONE-BLOCKING` — active wall damage and placement must publish tactical, radar, pointer-expiry, Recalc, zone, graph, owner, and retained-count effects inside each native wall transaction. Trigger: every accepted wall hit and every runtime wall placement; the cleanup-removal sub-branch additionally requires a pre-existing isolated damaged active-retail wall. Player effect: stale tactical/minimap cells, retained targets, delayed or wrong path authority, and owner/count visibility in the wrong transaction. Frequency: wall hits and placement are ordinary; cleanup auto-removal is active-data-conditional with no established shipped-map witness. [transaction 3 critic repair / BR-M04 preservation]
- `MILESTONE-BLOCKING` — every fresh Techno construction must consume and retain its one Scenario word, while generated Techno-binding projection and restore must not double-draw it. Trigger: valid authored base/upgrade objects, Post-Map starting forces, and ordinary runtime production/spawn, including a later failed Unlimbo. Player effect: later bridge damage/debris/repair randomness and constructor-word-driven report choices diverge; fixed-map low-Mark variants do not, because native Mark runs before authored Technos. Frequency: essentially every match, with authored-map impact on most stock maps and runtime impact whenever units are created. [P0 prerequisite]
- `COMPOUNDING` — authored Overlay rows must construct/defer/drain real load objects on the shared native-ID and registry timeline, while steep-slope survivors remain lifecycle-only. Trigger: every admitted format-active authored row; the shared drain itself runs on every fresh reader. Player effect: CellAnim/tile-Anim order and later native identities diverge, or a rejected row wrongly renders/saves; slope registry persistence alone is ordinarily invisible. Frequency: common on authored stock maps, with slope/wall/child-Anim branches content-conditional. [transaction 3 / BR-M04, M05]
- `MILESTONE-BLOCKING` — active RMG must emit traversable low decks and CABHUTs while preserving the two-run lifecycle and all three RNG owners. Trigger: every preview and every launch of a retail random map; deck production itself is active on types 3/4. Player effect: generated water regions lack intended connections, fixed-map Mark corrupts already-stamped generated decks, accepted maps differ from fresh `.SED` launches, or split/double constructor draws shift all later Scenario randomness. Frequency: every random-map session, with bridge placement on every qualifying generated map. [BR-M02]
- `COMPOUNDING` — RMG preview must keep its active native Building/Anim/latch/sound lifetime and numeric-ID reset separate from the UI candidate and from collision-free Rust handles. Trigger: every Generate, same-key or changed-key replacement, Cancel/re-entry, and accepted launch. Player effect: waterfall loops or preview generations start/stop at the wrong boundary, later constructor identities/order drift, or a same-key regeneration is changed by premature cleanup. Frequency: every random-map preview; audible impact occurs on candidates with admitted waterfall heads. [BR-M02 preservation, transaction 3]
- `COMPOUNDING` — exact topology/zone/hierarchy rebuild feeds all later path decisions. Trigger: load, collapse, repair and restore. Player effect: long-lived or post-load path divergence. Frequency: every topology mutation. [BR-M04, M10, M22]
- `MILESTONE-BLOCKING` — spawn, Unlimbo, factory exit, paradrop, landing and teleport must initialize the correct object plane, list, occupation bytes and height. Trigger: every bridge-adjacent creation or relayer. Player effect: a unit appears under the intended deck, occupies both planes, blocks the wrong route or renders at the wrong Z. Frequency: intermittent on bridge maps and common for factories/air insertion placed nearby. [BR-M08]
- `MILESTONE-BLOCKING` — direct target acquisition and weapon-specific gates must distinguish deck from under-span objects. Trigger: ordinary scans and attacks near high bridges. Player effect: units acquire or fire at invalid cross-plane targets. Frequency: common in combat near high bridges. [BR-M14, M16, M21]
- `COMPOUNDING` — scatter/crusher and stuck logic must use the selected plane. Trigger: moving blockers, crushing and stopped invalid occupancy. Player effect: wrong units scatter, teleport one cell, or self-damage. Frequency: intermittent but repeatable in traffic. [BR-M23]
- `COMPOUNDING` — projectile/Bounce collision and DBRIS fallout must use the bridge plane. Trigger: shots and collapse debris. Player effect: projectiles pass through decks or debris damages the wrong layer. Frequency: frequent during bridge combat/collapse. [BR-M14, M15, M18]
- `COMPOUNDING` — render consumers must use their exact raw facts and ordering. Trigger: every frame near bridges, ore/water sparkles, selected paths or Psychic Sensor lines. Player effect: occlusion seams, wrong rails/shadows, sparkling under decks or floating action lines. Frequency: continuously visible where applicable. [BR-M20]
- `PRESERVATION-BLOCKING` — TIBTRE-driven ore placement must continue rejecting raw structural/destroyed mask `0x500` rather than generalized walkability. Trigger: each `TIBTRE01..03` spread attempt near a bridge. Player effect: ore appears on structural or destroyed bridge cells and changes economy/pathing. Frequency: periodic on maps containing the stock spawning terrain. [BR-M24]
- `CONTENT-CONDITIONAL-BLOCKING` — valid explicit tubes must load, connect hierarchy, move and persist exactly. Trigger: a valid custom map with `[Tubes]`. Player effect: unusable or nondeterministic tunnel routes. Frequency: zero in scanned shipped maps, guaranteed when authored. [BR-M12, M22]
- `CONTENT-CONDITIONAL-BLOCKING` — tagged bridge collapse events/actions must reach only the verified footprint. Trigger: authored campaign/custom trigger content. Player effect: scripts fail or unrelated tags fire. Frequency: map-dependent. [BR-M18, M21]
- `COMPOUNDING` — snapshot/restore must apply each native persistence transformation and rebuild derived state; Scenario specifically becomes seed-zero rather than retaining its serialized cursor. Trigger: every save/load. Player effect: plane, path, debris or RNG divergence immediately or later. Frequency: every restored bridge-bearing game. [BR-M22]
- `RESOLVED-EXCLUSION` — TS-only, dormant, editor-only and name-only mechanisms remain excluded. Trigger: future code searches or OpenTS comparison. Player effect if violated: invented behavior and architecture drift. Frequency: development-time risk rather than retail runtime. [negative-fact ledger]

## Determinism and Persistence

- Scenario, Main and MapGen RNG streams remain distinct. Each closure contract names the stream and exact draw order.
- Every RMG call reconstructs MapGen from the stored map seed. Preview consumes and returns the
  process shell Scenario cursor; successful Start discards that cursor by constructing the gameplay
  Scenario/Main streams from the match seed before `.SED` regeneration.
- Native numeric IDs are a fourth independent deterministic stream, not collision-free Rust handles
  and not Scenario RNG. Every preview Generate resets that counter to `1,000,000` before its cleanup
  decision without rewinding Scenario; same-key replacement emits zero prefix Assign events and may
  immediately reuse `1,000,001` while retained Types/Houses/Supers/real-or-dummy Cells/Anims keep that
  numeric value on distinct runtime handles. Missing/changed storage advances through
  `R+P_preview+HB+K_preview` before the first generator object. Fresh `Full_Init` instead carries one
  cursor through the exact campaign/noncampaign constructor formula, snapshots `C_saved`, retains any
  shadowed theater events, installs wrapping `C_saved+0x2710` from the snapshot, then spends every
  allocated Tube source-row ID before parse and Overlay.
- Preview-native Buildings/Anims, retained cross-class native IDs, latches, sounds, counter, final
  growth/spread queues, and Scenario advancement survive Cancel
  and no-Generate re-entry even though presentation/storage owners do not. They are process-shell
  state, not gameplay snapshot authority; the next changed/missing-key full cleanup or gameplay
  `Full_Init` is their native destruction boundary.
- Authored load-object registries and the shared duplicate-permitting deferred queue preserve native
  insertion/live-drain order through the reader. Successful/wall Overlay records are destroyed before
  the first sweep; steep-slope survivors remain lifecycle-only until scene teardown. Matching native,
  those survivors are excluded from gameplay presentation/current-object checksum and snapshot
  reconstruction. Their spent native-ID cursor effect remains authoritative for later constructors;
  transaction 3 must preserve deterministic Rust continuation, while transaction 21/OQ-19 separately
  verifies the exact native save/restore transform rather than inferring it from survivor absence.
- Every valid active stock offline noncampaign launch owns one immutable
  `PreFillScenarioPrefixPlan`. It evaluates both H-sized House passes, both mode-family Gather calls,
  default-cell deficient retries and the zero-draw reset from the match-seeded `S0`; load validates
  the complete pre-state, installs the exact `S1` once, and then owns every downstream draw. A
  no-plan stock-offline fallthrough, one-House-pass plan, Battle-only plan, or Cooperative one-Gather
  path is explicitly nonconforming. Non-offline contexts use the separately proved typed matrix:
  campaign single House pass; LAN House1/`+0x80`/selected-`+0x84`/reset/House2; WOL
  House1/`+0x80`/common-assignment/reset/House2; replay inheritance; and save/generated no-Mark
  boundaries. None may substitute the offline prefix.
- Generation-time actual constructor events are authoritative even when a Neutral-Tech object is
  later discarded after failed placement. `RmgConstructionTrace` records every actual Building
  construction; a failed CABHUT site search precedes construction and is absent. A discarded
  Neutral-Tech event consumes its constructor word and native ID but has no binding.
  `GeneratedTechnoInitTable` binds the consumed low word and native ID for each emitted entity, and
  validated projection installs both without spending a second Scenario draw or native ID; its
  collision-free runtime handle remains an independent identity.
- P0 gives fixed authored-map, Post-Map and runtime constructors the same field from a direct draw
  in native order. It consumes before Unlimbo and therefore retains the cursor advance when
  placement later fails. Authored upgrades are distinct stable entities linked to their base and
  slot, so their constructor words persist for later Techno consumers, snapshots and hashes.
- Generated Techno-binding projection and snapshot restore install an existing constructor word and
  consume zero additional constructor draws. This does not suppress native generated tile-animation
  RandomRate draws at their staged Recalc boundaries.
- Generated low-deck overlay/data rectangles are direct materialization input, then generator-native
  Recalcs and final argument-1 germination/value finalization may mutate them. Only fixed authored
  low endpoints run `OverlayClass::Mark` and its Scenario draws.
- Fixed-range draws still advance the verified RNG stream.
- Linked-list/vector traversal order is retained where it controls outcomes: collapse fallout, scatter snapshots, repair selection and observer delivery.
- Presentation consumes no gameplay RNG and cannot mutate bridge state.
- New deterministic runtime state must be included in snapshots/hash only when native-active behavior persists across ticks. Derived topology is rebuilt at the verified restore boundary.
- Snapshot schema changes are isolated to the closure unit that introduces authoritative persistent state; old-version rejection and deterministic round trips are tested there.

## Validation Strategy

During transaction implementation and critic correction, use only focused `--lib` tests after
confirming no other session owns Cargo. Favor native-trace tables and small retail fixtures over
broad certification matrices. After the final fresh critic passes and before that transaction PR is
declared ready, run the full `cargo test -p vera20k --lib` exactly once for that PR.

Required fixture families:

- P0 constructor fixture family: fixed-map Unit/Aircraft/Infantry/Structure order, a
  constructed-then-failed placement, a pre-construction rejection, and a distinct linked structure
  upgrade; Post-Map starting MCV/extra-unit order including failure; ordinary placed and limbo
  runtime spawns; generated Techno-binding projection spending zero additional constructor draws;
  and snapshot round trip
  retaining words/upgrade links without a constructor draw;
- P0-R1 stock-offline prefix fixtures for all ids `1..9`, two identical native-roster House passes
  including observer and invalid-AI-slot cases, two independent Battle/Cooperative Gathers, sparse and
  deficient default-cell inputs, zero-draw reset, accepted-RMG staging provenance permitted only for
  Battle id `1`/FFA id `2`, rejection for other generated/mode combinations, exact full-state single
  installation, duplicate/tampered rejection, and draw-free later assignment projection; the first
  pass must leave no live `BasePlan`/AI/lifecycle/snapshot/hash state, the second pass must preserve
  current final-House state, and the reference cursor must match through Fill, applicable Mark or
  generated replay, authored/Post-Map work, and runtime `recalc_base_plan`;
- typed non-offline fresh-load-context fixtures: campaign single House pass and no Gather; LAN full
  House1/`+0x80`/selected-`+0x84`/reset/House2 sequence; WOL full House1/`+0x80`/common-assignment/
  reset/House2 sequence with both gated chooser arms; replay adds zero before its recorded family;
  an untyped generic fresh context at either missing/1 or format-active 4 cannot guess stock offline
  or a native-ID prefix; a typed authored missing/0/1-format fixture executes the full identity prefix
  and ungated sweeps but no pack Mark; and generated
  `.SED` retains the stock-offline prefix while its source provenance suppresses Mark; a separate persistence fixture
  proves restore has no fresh descriptor, Full_Init, Fill, prefix or Mark call and ends Scenario at
  seed zero;
- staged-owner fixtures prove `ScenarioBootstrapRng` is consumed before Fill into one real Simulation
  load runtime whose Scenario/native-ID/handle/registry/queue identities remain unchanged through
  Fill, authored pack, both Recalc boundaries, object sections, final payload installation, Post-Map,
  and gameplay. No map callback can target a not-yet-created sim, and no shadow lifecycle registry or
  end-of-load owner transfer/reconstruction is accepted;
- pure scheduler-root fixtures run after Fill but before OverlayPack/first Recalc in both production
  and headless paths. They discover every required terrain-Anim asset root yet consume zero native IDs,
  handles, Scenario/Main draws, registrations, sounds, latches, or overlay mutations. Missing assets
  fail before the first OverlayPack/Recalc Anim-construction effect while preserving already-spent
  prefix native IDs and Fill RNG state; after binding, every actual Anim is still constructed only by
  the synchronous sim-backed sink at its native Recalc boundary;
- authored low-Mark raw-seam fixtures bracketing full cursor states after prefix, after Fill, after
  exact `3*L` `raw & 3` writes, and before the first authored Techno; fixed/search/no-op/failure arms
  draw zero, and no ranged helper or cloned cursor is accepted;
- source mapping fixtures: Loose and Mix map to authored, with every fresh gameplay path requiring a
  typed family/native-ID receipt; missing/0/1 format skips only pack Mark while 2/4 enters Mark. Generated selects materialized/no-Mark
  even with missing or empty construction trace, then rejects missing phase transport instead of
  reconstructing history or falling back to authored; LegacyFallback rejects and untyped Generic
  rejects every fresh gameplay-equivalent load before identity/Mark effects rather than guessing. A
  separately named pure-map/no-live-effects diagnostic is non-parity; accepted generated start staging is distinct from both map source and trace,
  consumed once, retained as the active Scenario start table for loading markers and session/hash,
  and cannot be inferred from a cancelled/replaced preview, authored map, external `.SED`,
  regenerated waypoints, or generated construction events;
- production/headless/auxiliary equivalence fixtures pass the identical admission descriptor through
  each route and assert equal output, full Scenario cursor, native-ID cursor/event trace, and typed
  failure point for typed authored format 4, typed authored absent/1 with full identity prefix and no
  Mark draws, and generated materialized staged output with its required phase transport. Untyped
  Generic rejects before any native-ID/Mark/draw at every format; no selector-free constructor may
  certify a path production rejects;
- one interleaved authored OverlayPack fixture where an earlier low trigger writes cells and a later
  packed coordinate observes/overwrites them, including a high anchor in the same y/x traversal;
  assert the exact high save/setter-Recalc/anchor-restore window and ordinary `0 -> Land5:1 ->
  eight-neighbor germinated density -> Crate:FF` state order. Germination tests pin the exact
  `[0,1,3,4,6,7,8,10,11]` table, exact `N..NW` order, same-TiberiumClass rather than exact-id/state
  matching, Land-5/non-Tiberium early return at state `1`, flagged range-miss class-0 fallback,
  source-order packed overwrites, and a no-data `2x2` y/x fixture whose final rows are exactly
  `[0,1;3,4]`. Edge instrumentation proves every true miss reuses the shared dummy, its final stamped
  coordinate is the last true miss in N..NW order, later real hits do not clear that stamp, and the
  helper never writes dummy identity/state. Assert zero RNG, zero dirty/Recalc, and zero direct
  queue/bitmap/heap mutation; the no-OverlayData `2x2` retains density into queue initialization, all
  four cells pass the held-constant growth threshold, state `0` fails the spread-density gate, and
  states `1/3/4` pass it without recomputation. Instrument authored queue setup to prove growth then
  spread initialization occurs immediately after `[Terrain]`, observes any Terrain resource clear,
  and precedes every Unit/Aircraft/Infantry/Structure/Smudge. An adversarial flat resource cell that
  is spread-eligible at that boundary and receives a later ground occupier must remain in the seeded
  queue state: delaying initialization until after object occupancy or rebuilding after the final
  pass must fail. Instrument authored `InitCellAttributes(0)` to prove it calls zero germination
  helpers but invokes value-only `Get_Tiberium_Value` across the real-cell pass, contributes signed
  zero for non-resource cells and wrapping signed 32-bit `(existing_state + 1) * Value` for each
  recognized resource to its return total, then Recalcs without changing the held queue snapshot.
  Assert `Full_Init` stores that exact wrapping result to the MapClass `+0x134` analogue, it persists
  across the remaining load steps, and cell-array teardown resets it to zero; generated argument 1
  must not write it. Assert no invented gameplay/save/hash/presentation consumer. Then a
  conflicting OverlayData byte must win before the first anti-diagonal
  sweep validates identity and derives exact Road, LAT/CliffBack, zone and compact-cache state without
  reading the data byte. Assert exact identity/data through the map finalizer's separate payload
  and runtime `OverlayGrid`; the separate receipt is non-Clone, cannot be consumed twice, and poisoning
  or clearing the raw packs after finalization cannot affect sim or presentation. Then construct
  Terrain/authored objects and prove the post-object boundary scans current live Anim order and
  immediately scalar-deletes/unregisters the transient terrain animations, compacting survivors and
  releasing current sound handles without configured StopSound, ExpireAnim, or pending delete. Every
  producer owner remains null and no terrain Anim enters entity/cell occupancy. Then assert the
  transaction-3-owned `InitCellAttributes` sequence in exact order: one pre-loop ancillary raw-bit-
  clear slot; per-cell opaque-zero slot; cell-light cache invalidation/recompute-routing; latch clear; generic tag-line-
  restamp slot carrying `0x19`-before-`0x1A` precedence; argument-specific value work; Recalc; and post-
  Recalc wall-owner reconstruction for a current wall. Transaction 3 does not materialize the raw tag
  bits, their `FootClass`-equivalent consumer, semantic LightConvert/ZAdjust values, or a `+0x30` field.
  Instead, its fixture asserts exact slot/event order, one cache invalidation at the light slot, and
  negatives proving no `BridgeFacts`, bridge-zone, pathing, or invented opaque-field authority. The
  generic trigger owner later consumes the preserved clear/restamp/dual-event-precedence contract;
  transaction 20 separately proves the ordinary-versus-sentinel recomputation, semantic cell-light
  output, and no retained-preview cache leak;
  transaction 21 retains the `+0x30` raw-state question. The existing wall-owner implementation must reuse the final
  current identity rather than run before Recalc or through a duplicate owner. The Recalc recreates
  the surviving terrain-Anim set. An
  authored pair must distinguish an animation first created in per-Mark source order from one first
  created in the remaining anti-diagonal sweep and pin base registration, unique-ID and Anim-registry
  insertion before optional Scenario RandomRate; Reveal/Unlimbo and Logic with no occupancy;
  delay-zero Middle/StartSound before producer marker/ZAdjust/latch; conditional `Start` only when raw
  SHP frame-count/2 is zero; and unchanged Main RNG. A custom RandomRate tile pins the cursor path,
  all active stock TileAnim types pin the zero-RandomRate control while still consuming IDs and
  registration, a mixed-registry fixture pins unrelated survivor order, and a waterfall fixture pins
  `WA01X` StartSound/live-handle-release/final-restart while a non-`01` waterfall control emits no
  StartSound, with an explicit configured-StopSound negative. Mutate one first-generation-eligible
  tile/overlay through Terrain/object loading so it is deleted but not recreated, while an unchanged
  eligible peer recreates; final state must come from the live second Recalc, not cached descriptors. Poison
  otherwise eligible authored and generated cells with a missing referenced AnimType and forced Anim
  allocation/registration failure; both must abort explicitly before gameplay rather than silently
  dropping the generation. The
  final sim-owned grid must
  be the sole source for the app terrain template, occupied overlay render index, atlas/name closure,
  minimap/radar, and bridge presentation; a procedural body absent from raw entries must render, and
  a rejected/cleared raw row must not. Instrumentation must prove one
  object-level tactical dirty per accepted ephemeral row before derived dispatch (including slope
  rejection), helper argument `0` and no optional bridge-counter increment, zero repeated dirty for
  generated low cells, and zero second pack decode/Mark/filter. Separate admission controls cover
  missing/negative/0/1 format, non-`0xFF` decode, SHP-or-CellAnim, multiplayer crate rejection,
  radar-diamond edges, allocation failure, steep slope after base Mark, and positive OverlayData
  with empty/rejected identity;
- authored Overlay load-object lifecycle fixtures seed explicit `C_saved` and two successful Tube
  bindings, then assert four ordered base registry/listener joins -> native Overlay ID -> Overlay
  registry join -> direct base Unlimbo. A success with ordinary CellAnim plus an unlatch eligible
  terrain Anim must order `Overlay ID -> CellAnim ID -> terrain Anim ID -> next Overlay ID` while the
  prior success remains dead/queued/registered. Observe all successful dead objects in every joined
  registry through later identity rows and the entire OverlayData pass, then only the common epilogue
  removes them. Common success must order UnInit pointer-expiration #1 -> already-limbo no-op -> death/
  enqueue -> drain Release -> scalar-destructor pointer-expiration #2 while memberships still exist.
   A slope-admitted authored wall must instead complete its wall effects and use that same common two-
   broadcast tail because ScenarioInit is nonzero. A separate counter-zero non-authored control retains
   UnInit pointer-expiration #1 -> full Limbo/Destroy/Mark-remove pointer-expiration #2 -> death/enqueue
   -> drain Release -> scalar-destructor pointer-expiration #3. Slope failure remains alive/limbo/on-
   map/redraw/registered/unqueued and has
  no cell/Grid/GameEntity/Display/Logic/current-checksum/native-save/render membership. It remains in
  the same sim-backed load-lifecycle registry after load completion and is released only by scene
  teardown; no final-cell reconstruction or transient finalizer drop may pass. A shared-queue
  seed `[alive A, dead B, B, alive C, dead D]` preserves A/C, removes both B entries before one
  destructor, finalizes D, and processes shifted/live-appended successors without skipping. Format 1
  and format 4 absent/empty bodies still drain a seeded shared dead object before the first sweep;
  generated format 0 creates no authored Overlay/Mark/dirty/ID but runs the same drain. Every fresh
  Full_Init reader entry first asserts the shared queue is exactly `[]`; the synthetic nonempty seed is
  an isolated shared-drain mutation fixture, not a claimed pre-reader state. Reader rejects
  and allocation-null spend no handle/ID/registry effects and high allocation-null only no-op restores;
  injected base/Overlay registry or queue-growth failure hard-errors rather than completing normally.
  After the scalar broadcast, destruction orders Overlay registry -> game-active Limbo no-op -> type
  clear -> queue -> Object registry -> the three pointer-expiration/all-Abstract/Tag listener
  registries -> free, and never refunds IDs. A fresh two-common-success row fixture must evolve the
  reader-owned drain input from `[]` to exactly `[overlay0, overlay1]` in source order and assert that
  no House/Super/Type/Cell/Tube prefix handle appears in that shared queue and the consumed prefix
  receipt allocates no Overlay-lifecycle runtime handle before `overlay0`.
  OQ-34's integrated absolute fixture derives `C_saved` from the complete prefix before asserting
  Tube/Overlay/child-Anim identities;
- low-Mark adversarial geometry fixtures for adjacent endpoints, first of two exact opposites, wrong
  ID/state pass-through, occupied fixed-row successful no-op, missing opposite partial fixed end,
  occupied/missing body overwrite, and edge lookups aliasing one persistent extended dummy in exact
  row/j order while preserving draw count and return/tail behavior; dedicated dummy checks prove a
  negative-i16 component can resolve a real fixed-stride slot before fallback, true misses retain
  identity/state, Resize resets `-1/0` without replacing identity, OverlayData never writes the
  dummy, and real edge LAT reads its `0xFFFF` tile sentinel with flat slope;
- Lost Lake and Killer: intact low crossings;
- Bay of Pigs and Hills: high deck, under-span, dual-plane and AttackMove;
- Deadman's Ridge: high collapse gap;
- Shrapnel Mountain: destroyed low bridge;
- deterministic RMG type 3/4 preview/cancel/accept/launch sequence asserting fresh MapGen state on
  each run; first setup entry with seed `-1` taking one shell seed draw and re-entry taking none; and
  continuing shell Scenario RNG cursor across repeated previews and Cancel. Preview fixtures must pin
  all thirteen native lifecycle cases, using OQ-34's branch-specific post-manual-setup cursor:
  argument-1 `Set_Defaults`/manual setup with no `Full_Init`, `Clear_Scene`, or `+0x2710` map-read
  transform; per-Generate native-ID reset to `1,000,000` before cleanup with no Scenario rewind;
  matching-key zero setup constructors and first-new-object `1,000,001` while retained Type/House/
  Super/real-and-dummy-Cell/Anim IDs may collide; changed/missing row-major one-ID-per-real-Size-diamond-
  Cell plus dummy-Cell-last, source-ordered Type, House/Super, and custom-theater prefix before the first
  generator object, with `R+P_preview+HB+K_preview` and retail `K_preview=0`;
  constructed-then-discarded Neutral Building word then ID/no refund; failed CABHUT
  pre-search spending neither; stock TileAnim ID/no-RandomRate and only four `*01X` WaterfallLoop
  attempts; custom RandomRate after native ID/registration with an independent collision-free handle;
  transient terminal Anim churn spending fresh IDs; same-key old Anim/latch/sound retention through
  intermediate Recalcs and legal temporary duplicate numeric IDs; changed-key old sound/Anim cleanup
  before the first new constructor but after reset; every Generate freeing spread then growth before
  reset and rebuilding growth then spread after generation; Cancel plus no-Generate re-entry retaining
  native objects/sounds/counter/queues/Scenario state after UI/snapshot destruction; first later Generate taking
  reset -> missing-snapshot full cleanup -> new construction; and acceptance alone retaining preview
  state until the gameplay `Full_Init` cleanup. Use Map with a preview performs no third Generate and
  Use Map without one performs exactly one; `.img` versus `.SED` commit gates remain exact;
- a preview-prestate poison fixture starts with one retained old latch, proves it suppresses the
  corresponding intermediate Recalc Anim, then proves terminal deletion/unlatch recreates it; applying
  a journal against a changed lifecycle generation token/prestate fails rather than guessing;
- fresh authored and accepted-`.SED` controls begin at Clear_Scene's `1,000,000`, replay OQ-34's
  exact campaign or `E_multi -> House/Super pass 1 -> Resize 1 -> P -> House/Super pass 2 -> Resize 2`
  constructor trace to derive `C_saved`, retain a custom shadowed theater Assign, then set wrapping
  `C_saved + 0x2710` from the snapshot. Assert the 1,704-row/1,699-explicit-constructor retail subtotal
  is not used as `P`. Each allocated successful Tube row spends before parse and gets a source-record-
  keyed `TubeNativeInit`; an allocated malformed row spends then hard-errors, while allocation-null
  spends zero then hard-errors. Neither continues to Overlay. Transaction-5
  installation reuses each binding with zero second native-ID allocation; preview proves this map-read
  prefix absent. Pin the absolute first-Overlay oracle as
  `O1 = wrap32(C_saved + 10_000 + T + 1)` for `T=0`, `T=2`, and a signed-32-bit wrap boundary, while
  preserving every synchronous child-Anim allocation before the next Overlay. Pin the report's
  concrete numeric oracles: `C_saved=1,000,018` installs map-read cursor `1,010,018`;
  `C_saved=1,000,037,T=0` makes `O1=1,010,038`; preview setup cursor `1,000,018` makes its first new
  object `1,000,019`; and cursor bit pattern `0xFFFFFFF0` installs `0x00002700` rather than adding to
  a changed current cursor. Successful Start
  reseeds Scenario/Main, unconditionally regenerates `.SED`, and carries one
  launch cursor through the complete stock-offline pre-Fill prefix, Fill, ordered construction replay,
  projection, Post-Map, and Simulation. The generated fixture records complete constructor-event
  order including failures; discarded Neutral-Tech consumes word then native ID with no binding,
  emitted projection reuses word/native ID with no double draw or native-ID allocation, and the
  collision-free runtime handle remains independent. It pins stored `Techno+0x3C8` per CABHUT,
  final MapGen/gameplay-Scenario continuations, direct generated low-deck identity/data with no
  authored Mark/low-Mark draw, and generated animation staging in exact order: any actual launch-only
  synthetic-Full_Init generation, actual CABHUT constructor effects, first generator Recalc
  animations, every emitted/discarded Neutral-Tech constructor, later Recalc/paint animations, final
  scalar deletion, then anti-diagonal per-real-cell latch clear, `SpreadCellGerminate(0)` returned-
  value aggregation, and Recalc/recreation. A generated resource-state fixture pins helper early-zero
  behavior, same-class/dummy density, each signed 32-bit `(new_state + 1) * Value` return, wrapping
  signed 32-bit local-sum arithmetic, and final payload state while proving queues retain
  their earlier initialization with no rebuild and no persistent aggregate. It also pins native-ID
  and runtime-handle order, custom RandomRate Scenario cursor, unchanged Main RNG, live-registry/
  sound order, and the stock-waterfall zero-RandomRate control; final cells or a flat complete
  Building trace cannot reconstruct earlier generations. The accepted-`.SED` boundary separately
  asserts two consecutive free-spread-then-growth pairs—generator entry, then
  `Full_Init`/`Clear_Scene`—followed by `Full_Init`'s single rebuild-growth-then-spread, with no preview
  payload promoted into gameplay;
- `BuildRiverBridge` negative fixture proving waterfall shaping emits no structural/low-overlay bridge topology;
- sealed valid custom `[Tubes]` map;
- automatic same-cell TubeClass shell classification and zero-length non-traversal case;
- spawn/Unlimbo/factory/landing/teleport plane/list/height cases, including a refused-placement negative path;
- CABHUT collapse/repair and tagged event/action cases;
- destruction-authority matrix covering campaign/editor map `[SpecialFlags]`, skirmish/multiplayer session `BridgeDestruction`, conflicting `[CombatDamage] DestroyableBridges`, weapon admission, and CABHUT/attached-bomb bypass;
- DBRIS bounce/landing damage;
- V3WH/V3EWH/DMISLWH AoE tolerance cases plus native-derived Rocker admission, 7x7 enumeration, numeric attenuation, timer jitter, selected-plane and dispatch-order oracles;
- TIBTRE ore placement and raw-mask negative cases;
- selected and Psychic Sensor action lines;
- snapshot restore across active movement, collapse debris and repair, asserting the complete
  seed-zero Scenario poststate and separately contracted Main/MapGen behavior rather than unchanged
  generic RNG continuation.

The full suite is a per-transaction PR readiness gate, not a per-edit or per-critic-iteration tool.
Each transaction records its one final literal full-library result after focused validation and its
fresh critic chain. A later dependent transaction begins only after that PR merges into `main`.

## Bridge-Wide Reverse Audit

After P0 and all bridge-mechanism passes:

1. Start from each active native writer/consumer and prove a Rust owner, exact test or evidence-backed exclusion.
2. Start from every Rust bridge field, helper, ignored test, approximation marker and branch and prove current active-retail authority or remove/correct it.
3. Re-run the OpenTS correspondence ledger as leads and confirm no active YR mechanism disappeared between unit boundaries.
4. Recheck all 27 mechanism rows, all 38 living questions, and every entry in the complete frozen negative-fact ledger; every open item must be resolved and every exclusion preserved, not deferred.
5. Re-run named retail fixture traces for load, move, target, damage, collapse, repair, render and restore.
6. Reconcile every merged transaction's full-suite certification and current `main` ancestry, update only verified System Map connections, and produce the final handoff. If the audit discovers any code correction, route it through a new dependency-coherent transaction, fresh critic chain, one PR-readiness full-suite run, and merge before repeating the affected audit rows.

Any omission or regression reopens its owning unit and requires the builder/fresh-critic loop again.

## Adversarial Design Questions

### Why not close the four GSI rows independently?

Because active consumers cross their nominal ownership. High topology feeds combat, presentation and save/load; low collapse feeds ordinary Road pathing; TubeClass feeds hierarchy and persistence; CABHUT transactions feed events, audio and repair. Independent row closure would recreate the omissions found by the ten-pass coverage audit.

### What could make ordinary skirmish still feel wrong after many green unit tests?

Wrong-plane movement smoothing, target acquisition, synchronous collapse fallout, topology rebuild and render ordering. These are common, cross-owner paths whose effects compound. The design gives each a named closure unit and then requires a final end-to-end reverse audit.

### What decision would cause the most expensive rework later?

Collapsing raw structural state, mutable walkability and object `OnBridge` into one generalized predicate. It would infect pathing, damage, PixelFX, action lines and persistence with subtly different semantics. The chosen design keeps them explicit at the start.

### Could the critic process itself become ceremonial?

Yes, if critics receive only a summary or builder-selected tests. The required bundle includes the complete requirement, native evidence, exact diff and literal output; critics are read-only, did not build the unit, and must recheck prior fixes. A pass cannot waive an open evidence term.

### Could dependency sequencing hide a prerequisite expansion?

Yes. A builder may promote only the smallest missing prerequisite necessary for the current unit, must record it in that unit's contract, and cannot start the prerequisite's wider backlog. Cross-unit discoveries reopen the coverage map instead of being absorbed silently.

## Approval Gate

The preferred approach is distributed exact-delta closure in current owners. It is approved only after a separate design-review pass verifies:

- every behavior claim is cited to the frozen native/retail coverage report, the closed dual-RNG
  lifecycle/follow-up report at `7fee6929`, `4a63fa15`, `f1e6054b`, and `a776f270`, P0-R1, the three
  settled low-Mark reports, the nine original 2026-08-31 transaction-3 reports, and the 2026-09-01
  authored-wall report in Sources, or explicitly
  open;
- all 27 mechanisms map to explicit mechanism gates and contributing implementation transactions;
- all 38 living questions route to a pre-implementation owner, evidence-backed exclusion, or final audit;
- no chosen interface collapses distinct native facts;
- the player-experience ledger covers ordinary high/low traversal, combat, collapse/repair, RMG, presentation, content-conditional tubes/triggers and restore;
- the critic protocol cannot close a mechanism with unresolved evidence;
- the universal stock-offline pre-Fill substitution includes both House passes, both mode-family
  Gather calls, exact default-cell deficient retries, zero-draw reset and explicit generated-staging
  provenance limited to accepted Battle/FFA on one native Scenario stream, and cannot install stale
  state, install twice, or leave a parallel downstream owner; its normalized roster preserves native-
  priority human nodes including observers, AI slot identity/validity, Neutral and Special without an
  `opponents.len()` reconstruction; pass 1 leaks no final House state, pass 2 preserves current
  `BasePlan`/AI/lifecycle/snapshot/hash state, and the cursor oracle reaches runtime BasePlan draws;
- the app-owned fresh-load-context descriptor is orthogonal to physical `LoadedMapSource` and has no
  restore variant; persistence owns a separate no-Mark restore context with no conversion into the
  fresh pipeline; Loose/Mix map
  to authored, Generated maps to materialized even without a trace, LegacyFallback rejects,
  and untyped Generic rejects every fresh gameplay-equivalent format before native-ID/Mark/draw
  effects. A named no-live-effects pure-map diagnostic is explicitly non-parity. Normalized prefix
  inputs enter `sim` without app/network dependencies; production, headless, and auxiliary
  constructors share that exact admission descriptor/function and output/Scenario/native-ID cursor/
  event-trace/error contract rather than defaulting to authored;
- one y/x OverlayPack traversal interleaves high/low/ordinary Mark, followed by the complete
  OverlayData pass and an ungated anti-diagonal Recalc before Terrain/Technos; its map-native
  post-validation identity/data/authored-blocker-count payload transfers consumed-once into runtime
  `OverlayGrid` and global count state without a second pack decode, Mark, filter, or final-wall scan.
  Every allocated row retains exact four-base-registry -> native-
  ID -> Overlay-registry -> direct-base-Unlimbo order; success/dead objects and child Anim ID effects
  remain registry-visible through later rows and data; slope-admitted authored walls complete their
  effects and queue through the common tail; slope failures survive only in non-presentation lifecycle
  registries. Generic counter-zero wall failure is separately preserved. The shared live duplicate-
  aware drain runs once after
  data/temp cleanup and outside format/body gates, including generated format 0, before the first
  sweep; hard errors replace partial registry/queue-growth degradation. Authored terrain animations retain per-Mark versus first-sweep
  construction order, ID/registry-before-RandomRate, delay-zero Middle-before-producer writes, no
  Main RNG or entity occupancy. Authored growth-then-spread queue initialization occurs from the
  post-Terrain/pre-object live map and remains unchanged through later occupancy and post-object
  InitCellAttributes's immediate scalar-delete/live-handle-release/no-StopSound/no-pending-delete;
  routed ancillary raw-clear/opaque-zero/light-invalidation/tag-line slots around the owned unlatch,
  value-only wrapping signed-32-bit total, Recalc, and current-wall owner reconstruction;
  `MapClass+0x134` persistent write; and Anim recreate lifecycle. The raw tag bits are explicitly not
  bridge-zone topology, lighting remains transaction-20 presentation authority, teardown resets the
  opaque total field, raw tag semantics and `+0x30` remain externally routed, and no unproved
  consumer or field is invented. Generated
  source skips authored Mark but preserves synthetic Full_Init state-dependent lifecycle plus every
  native generator Recalc boundary interleaved with constructed CABHUTs and all Neutral-Tech
  constructors. Generated queues initialize before final argument-1 InitCellAttributes, which clears
  each latch, performs the exact helper's early-zero or wrapping signed-32-bit
  `(new_state + 1) * Value` return and local aggregation, then Recalcs without a
  queue rebuild; a flat trace plus final cells is insufficient. Only final sim-owned terrain and OverlayGrid may seed the app
  template and presentation occupancy/assets, including procedural rows missing from the raw pack;
- native numeric IDs and collision-free runtime handles remain independent. Preview uses argument-1
  Set_Defaults/manual setup, resets its ID counter to `1,000,000` before each cleanup decision, keeps
  Scenario RNG continuous, preserves active shell objects/Anims/latches/sounds across Cancel, and
  implements both exact storage-key replacement branches and free-spread/growth then rebuild-growth/
  spread lifetime. Matching storage skips every setup constructor and may give the first new object
  `1,000,001` while a retained Cell owns the same value; changed/missing storage consumes
  `R+P_preview+HB+K_preview`, with row-major real Cells plus dummy last and retail `K_preview=0`.
  Every fresh `Full_Init` carries the same independent native-ID cursor from Clear_Scene through its
  exact campaign or noncampaign constructor formula, shadowable theater window, set-from-snapshot
  wrapping `C_saved + 0x2710`, allocated Tube rows, empty shared-queue prefix, and later authored/
  generated construction;
  accepted preview state is cleaned only by that launch boundary and never becomes gameplay authority;
- app retains the one Scenario borrow after Fill and backs a map-native raw-call interface, so `map`
  imports no `sim` type; the same cursor returns before authored Technos with no ranged substitute,
  clone or reseed, and edge writes alias one extended persistent shared dummy;
- restore asserts Scenario's verified seed-zero poststate while leaving Main/MapGen explicitly
  OQ-19-gated rather than claiming generic unchanged continuation;
- each dependency-coherent transaction owns one short-lived branch, builder, fresh-critic chain,
  one final full `--lib` PR-readiness run, and merge before dependent work; split mechanism rows
  remain open until their cumulative final-contributor review;
- P0 has a bounded all-ingress constructor requirement and its merged builder/fresh-critic evidence;
  P0-R1's merged universal stock-offline prefix correction and fresh critic pass before transaction 3
  uses that shared Scenario cursor; and transaction 3 preserves the completed campaign/LAN/WOL/replay/
  save/generated/editor context matrix without an untyped offline fallback; BR-M04's persistent
  builder and eventual full critic bundle cover both its transaction-3 shared high-load contribution
  and transaction 4 rather than inheriting BR-M05's pass.

Revision 18 passed a fresh read-only whole-document review after every prior correction; critic 4
returned PASS with zero material blockers on `origin/main@50e4b7ba`.
Revision 19 reopens only the authored-wall portion on newer native evidence. The focused wall report
passed its fourth serial read-only critic after compact-ID and active-winner census corrections; this
amendment replaces authored wall rejection with ScenarioInit-forced success and adds the retained
real-cell blocker-neighbor plane. It does not waive a fresh implementation critic.
Revision 20 follows the first fresh implementation critic's `NEEDS_FIX` verdict on `95f77159`.
Autonomous adversarial review approves the synchronous host repair because the three live-decompiled
transactions establish every material order boundary, the host extends an existing ownership seam,
and it changes no overlay predicate, RNG, snapshot, or hash authority. The highest remaining ways an
ordinary match could still feel wrong are one missed adapter, a retained hit without its pre-write
tactical dirty, or placement cleanup finishing after the next filler; each has an explicit production
fixture. Choosing deferred effect replay or a Simulation-owned wall service would cause the most
expensive rework, so both are forbidden. This self-approval satisfies the autonomous design gate but
does not waive the required new fresh read-only critic, who must recheck the original findings and the
complete correction.
Revision 21 records PR #207's landing. Its full-slice critic 3 decompiled the reader, Mark, wall
cleanup, `IsWallConnectableInDirection`, the cell iterator, `InitCellAttributes`, the
`AssignUniqueID` constructor roster, and the merge's two rules-processing claims live, returned PASS
with zero blocking findings, and opened OQ-37. The residual ledger above is the transaction-3
continuation's contract input; the next slice starts from refreshed `main` with G10/G11, the
`MapClass+0x134` aggregate, the ancillary seam, the `None`-plane gate, and OQ-37.
Revision 22 records PR #211's landing (slice B: OQ-37 and the `MapClass+0x134` aggregate IMPLEMENTED).
Its critic 1 returned NEEDS_FIX on the generated-launch particle-ID position, the missing `HideIfNoOre`
consumer, and the unsigned chance bounds; critic 2 rechecked every correction against the live binary
and returned PASS with one residual applied verbatim post-pass. The next slice starts from refreshed
`main` with G10's generator-tail germination and queue order.
Revision 23 records PR #213's landing (slice C: G10's generator-tail germination and queue order
IMPLEMENTED; OQ-38 opened with live evidence). Its critic 1 returned PASS with three residuals recorded
post-pass. The next slice starts from refreshed `main` with OQ-38's queue rebuild parity (`CellIterator`
insertion order, binary-heap pop order, percentage threshold, occupier gate).
These review passes close no bridge mechanism and waive none of transaction 3's implementation contract,
builder/fresh-critic chain, focused validation, PR-readiness full suite, or merge gate.

## Sources

- `docs/research/bridges/00-system-models/ACTIVE_RETAIL_BRIDGE_COVERAGE_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/00-system-models/RMG_BRIDGE_DUAL_RNG_LIFECYCLE_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/00-system-models/RMG_PREVIEW_ANIM_BUILDING_IDENTITY_LIFECYCLE_REINVESTIGATION_GHIDRA_REPORT.md`
- [docs/research/bridges/00-system-models/SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/SCENARIO_PREFIX_PLAN_INELIGIBLE_FALLBACK_REINVESTIGATION_GHIDRA_REPORT.md)
- `docs/research/bridges/01-assets-map-load-overlay/LOW_OVERLAY_MARK_FIXED_MAP_STAMP_RNG_TRANSACTION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/LOW_OVERLAY_MARK_SCENARIO_LOAD_ACTIVATION_BOUNDARY_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/LOW_OVERLAY_MARK_ALL_LOAD_CONTEXT_SCENARIO_RNG_LIFECYCLE_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAYPACK_INLINE_TRANSACTION_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAY_EPHEMERAL_OBJECT_FINALIZATION_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAY_WALL_SCENARIOINIT_ACCEPTANCE_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/CELL_0X122_DYNAMIC_BLOCKER_LIFECYCLE_RUST_MAPPING_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_TIBERIUM_GERMINATE_SIDE_EFFECT_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/TERRAIN_ATTACHED_ANIM_LOAD_LIFECYCLE_SIDE_EFFECTS_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/OVERLAYPACK_SHARED_DUMMY_FINAL_RECALC_FIELDS_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_MARK_LOAD_CONTEXT_SOURCE_PROVENANCE_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/FULL_INIT_AND_PREVIEW_NATIVE_ID_PREFIX_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/bridges/01-assets-map-load-overlay/INITCELLATTRIBUTES_TAG_LINE_LIGHTING_TAIL_REINVESTIGATION_GHIDRA_REPORT.md`
- `docs/research/CELLCLASS_STRUCT_GHIDRA_REPORT.md` (`CellClass::Get_Tiberium_Value @ 0x00485020`)
- `docs/research/miner/HARVESTER_MISSION_HARVEST_GHIDRA_REPORT.md` (2026-07-24 live retail
  `Get_Tiberium_Value @ 0x00485020` zero/formula recheck)
- `docs/research/LOADING_FULL_INIT_PROGRESS_SEQUENCE_AFTER_00552D60_GHIDRA_REPORT.md`
- `docs/research/MAPCLASS_GHIDRA_REPORT_REVISIT_2026_04_24.md` (`MapClass+0x134` write/reset xrefs)
- `docs/research/skirmish-ui/RMG_TERRAIN_SHAPING_CORE_GHIDRA_REPORT.md`
- `docs/research/skirmish-ui/SKIRMISH_RANDOM_MAP_GENERATOR_00598960_GHIDRA_REPORT.md`
- active `gamemd.exe` addresses and retail inputs enumerated by those reports
- current Rust owners at freshly fetched `origin/main` snapshot
  `50e4b7ba4732fd3fb48e5b819e1abc55327ec557`
- [docs/system-map/topology.v2.json](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/system-map/topology.v2.json) `bridge-helpers` service boundary
- `C:\Users\enok\Documents\OpenTS` correspondence ledger, as navigation leads only
