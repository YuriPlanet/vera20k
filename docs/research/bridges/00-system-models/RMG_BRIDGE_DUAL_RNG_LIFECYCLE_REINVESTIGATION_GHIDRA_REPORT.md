# RMG Bridge Dual-RNG Preview / Accept / `.SED` Launch Lifecycle — Ghidra Re-investigation

**Address(es):** `0x00595BC0`, `0x00596300`, `0x00598960`, `0x005904B0`,
`0x006F3254`, `0x005E8590`, `0x0052FC20`, `0x0052E619`, `0x0052E745`,
`0x00683AB0`, `0x00684620`, `0x00684989`, `0x00686B20`, `0x00743270`,
`0x0041B110`, `0x0051FB00`, `0x0044F820`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** One-process retail YR random-map lifecycle as it affects active bridge
generation: setup entry, repeated preview generation, setup Cancel, setup Use Map, `RandMap.img`
and `RandMap.Sed`, launch-time `.SED` reload, `g_MapGenRng`, the front-end/gameplay Scenario
RNG, and CABHUT construction in the shared RMG object-constructor sequence.
The design-review follow-up also covers the minimum authored-Techno constructor ordering needed to
keep that shared gameplay Scenario cursor exact before later bridge RNG consumers.
**Non-Scope:** Full RMG terrain formulas, Main-RNG option-randomization formulas already owned by
the RMG option reports, complete shell paint parity, malformed external `.SED` UX, and unrelated
post-load gameplay RNG consumers.
**Confidence:** High. Control-flow, RNG-object identity, copy sizes, call order, and bridge-hut
constructor consumption are direct active-binary evidence. The preview/cancel half also has a
same-process retail memory/file trace.
**Active in YR:** Conditional but active retail behavior. It is reached from the stock offline
Skirmish Choose Map `Create Random Map` command and from any selected filename ending in `.SED`.

## 0. Working Notes Gate

- **Target question:** Does native retain the preview-generated map and one RNG continuation into
  play, or does it persist seed/options and regenerate; which RNG owns each draw, and what must a
  bridge implementation preserve for CABHUT construction?
- **Prior state:** `MAPGEN_SAME_PROCESS_LIFECYCLE_BRIDGE_CALLER_RECONCILIATION_GHIDRA_REPORT.md`
  was partial and explicitly left preview/accept/launch runtime questions. The skirmish `0x583`
  reports proved the modal transaction but did not reconcile it with the current Rust retained-map
  shortcut or the Scenario RNG.
- **Evidence needed to close:** exact setup control ids and result gate; exact seed/copy behavior at
  every generator call; direct Scenario-RNG constructor site; preview/cancel persistence; accept
  file order; successful-Start reseed ordering; `.SED` reader/generator call; current Rust owner
  scan; and a negative-fact list preventing a retained-preview interpretation.
- **Stop condition:** all load-bearing lifecycle questions resolved with no OPEN item. Visual pixel
  composition and non-bridge RMG formula details remain with their existing owners.

## 1. Verdict

Native does **not** carry a preview-generated scenario into gameplay. Every call to
`RandomMapGenerator__Generate @ 0x00598960` constructs a fresh local `RandomClass` from
`MapSeed+0x74`, then copies all `0xFD` dwords into `g_MapGenRng @ 0x00ABE890`. Repeated Generate
therefore restarts MapGen from the current seed; it never continues from the preceding preview.

Preview generation nevertheless mutates a second stream: the process Scenario RNG at
`(*0x00A8B230)+0x218`. `TechnoClass__Constructor @ 0x006F3254` takes one raw
`Random__Next @ 0x0065C780` from that object and stores its low word at `Techno+0x3C8`.
`PlaceBridgeRepairHut @ 0x005904B0` reaches that constructor once for every allocated CABHUT.
Other RMG building-placement owners reach the same constructor, including attempts later
destroyed, so an observed Scenario cursor delta is the full constructor sequence, not the count of
surviving CABHUTs or structures.

Cancel preserves both advanced RNG objects in the process and, after a preview exists, writes the
temporary surface to `RandMap.img`; it does not write `RandMap.Sed` or commit the sentinel. Use Map
`0x6C5` short-circuits directly to result `1` when a preview exists, so it consumes no third
generation. The setup runner then writes `RandMap.img`, and the accepted caller writes
`RandMap.Sed` and commits the ordinary sentinel record.

Successful Start later calls `Init_Random_Number_System @ 0x0052E619`, replacing both Scenario
and Main RNG objects from the match seed. The skirmish branch then reaches
`ScenarioClass__Start_Scenario @ 0x0052E745`; `.SED` detection in
`ScenarioClass__Read_Scenario @ 0x00684620` reloads the seed/options and calls
`0x00598960(0,0)` at `0x00684989`. Thus gameplay performs a second complete deterministic RMG
run from the `.SED`, after the Scenario reseed. Preview-time Scenario draws do not carry into the
match; launch-time constructor draws do. “After the reseed” does not mean the first CABHUT sees
the pristine seed cursor: launch-mode generator initialization calls `ScenarioClass__Full_Init`
before bridge/tech placement, so the same Scenario object first carries the native Full-Init
house/start/map-Fill prefix.

## 2. Authoritative State and Offsets

| State | Native owner / address | Exact role | Active in YR |
|---|---|---|---|
| `MapSeedClass` | `0x00ABDFD8` | Persistent setup options; seed at `+0x74` | Conditional RMG |
| `g_MapGenRng` | `0x00ABE890`, `0x3F4` bytes | R250 object replaced at the start of every generator call | Conditional RMG |
| Scenario RNG | `(*0x00A8B230)+0x218`, `0x3F4` bytes | Process shell cursor; replaced from match seed on successful Start; gameplay cursor thereafter | Yes |
| `g_MainRng` | `0x00886B88`, `0x3F4` bytes | RMG Randomize/derived-option draws; seeded beside Scenario, but distinct | Yes |
| temporary setup preview | `DAT_00ABE154` | Generated surface wrapper; serialized to `RandMap.img` during setup teardown | Conditional RMG |
| setup MapSeed clone | `DAT_00ABE150` | Snapshot of the options behind the current preview; destroyed on setup close | Conditional RMG |
| chooser preview | `DAT_00AC1154` | Rebuilt from `RandMap.img` only after accepted setup | Conditional RMG |
| seed file | `RandMap.Sed` | `[RandomMap]` options persisted only after accepted result `1` | Conditional RMG |
| low bridge repair hut | `CABHUT`, Neutral | Live `BuildingClass` created by `0x005904B0`; one raw Scenario draw in base Techno constructor | Conditional low bridge |

`RandomClass` layout is `byte locked @ +0`, three non-semantic padding bytes, indices at `+4/+8`,
and 250 state words at `+0x0C..+0x3F3`. `Random__Seed @ 0x0065C6D0` initializes indices to
`0/103`. Native full-object copies are `0xFD` dwords (`0x3F4` bytes); the padding can contain
stack residue and must not be treated as a logical draw-state difference.

## 3. Native Control and Lifecycle Proof

### 3.1 Setup button mapping and entry seed

`RandomMapSetupDialog__Proc @ 0x00596300` proves the exact command mapping:

| Decimal | Hex | Command |
|---:|---:|---|
| 1730 | `0x6C2` | Load Map |
| 1731 | `0x6C3` | Save Map |
| 1732 | `0x6C4` | Delete Map |
| 1733 | `0x6C5` | Use Map / OK |
| 1472 | `0x5C0` | Cancel |
| 1568 | `0x620` | Generate Map |
| 1569 | `0x621` | Randomize |

On `WM_INITDIALOG`, only `MapSeed+0x74 == -1` draws a seed. Assembly
`0x00596BB1..0x00596BC8` loads `ScenarioClass+0x218`, calls inclusive
`RandomRanged(0,0xFFFF)`, stores the result at `0x00ABE04C`, and marks options dirty.
Because the global MapSeed survives setup close, a later reopen with a non-negative seed performs
no second entry draw.

The Randomize command is a separate stream. Assembly at `0x0059678A`, `0x005967A4`,
`0x005967B2`, `0x005967C5`, `0x005967D8`, and `0x00596826` loads
`ECX=0x00886B88` before `Random__RandomRanged`. `MapSeedClass__RandomizeDerivedFields @
0x00597260` likewise uses `g_MainRng`. Do not route option randomization through either
MapGen or Scenario.

### 3.2 Every Generate replaces MapGen from `MapSeed+0x74`

At generator entry:

1. `0x0059897B` reads `MapSeed+0x74`.
2. `0x00598980..0x00598985` constructs/seeds a stack `RandomClass` with that value.
3. `0x0059898A` loads count `0xFD`.
4. `0x00598996` loads destination `0x00ABE890`.
5. `0x0059899B REP MOVSD` replaces the complete global object.

No caller supplies a continuation. The operation is identical for dialog Generate
`0x00598960(1,hDlg)`, OK-without-preview generation, and launch generation
`0x00598960(0,0)`.

### 3.3 Preview-time Scenario consumption is construction-side, not map-choice RNG

All direct RMG choice sites verified in the existing RMG reports load `g_MapGenRng`. The
cross-stream exception is object construction:

```text
006F3249  MOV EAX,[0x00A8B230]
006F324E  LEA ECX,[EAX+0x218]
006F3254  CALL 0x0065C780       ; raw Random__Next
006F3259  MOV word [ESI+0x3C8],AX
```

For bridge huts, `PlaceBridgeRepairHut @ 0x005904B0` scans the supplied inclusive rectangle,
allocates one `BuildingClass` only after a cell qualifies, and Unlimbos `CABHUT` for Neutral.
The helper returns after that one allocation. `PlaceLowBridgeDeck @ 0x0058F2C0` invokes the helper
for each end, trying a fallback rectangle only after a primary failure. Therefore every allocated
CABHUT consumes exactly one raw Scenario word, in deck/end execution order, and a successful deck
can contribute up to two such draws.

The complete `BuildingClass__Constructor @ 0x0043B740` caller census contains
`0x005904B0` (CABHUT), `0x00595400` and `0x005A95B0` (neutral-tech placement), plus dormant
RMG-shaped helpers `0x005A6510`, `0x005A82E0`, and `0x005A91E0`. Active generator reachability
narrows the event set. `0x005A6510` and `0x005A82E0` are called only by `0x005A5020`, while
`0x005A5020` has no code or data xref and its little-endian entry address does not occur as a
pointer in the image. `0x005A91E0` likewise has no code/data xref or pointer occurrence. None is
in the transitive call closure of `RandomMapGenerator__Generate @ 0x00598960`; they are therefore
excluded from the active-retail construction trace. The active trace has only CABHUT and the two
neutral-tech owners.

Within the active neutral-tech owners, construction precedes some placement-attempt loops; a
failed attempt may delete the object only after its Techno constructor already spent the Scenario
word. Consequently:

- the Scenario delta is not a MapGen decision input;
- it is not equal to the final `[Structures]` count;
- it is not safe to reconstruct by counting only surviving CABHUTs;
- exact parity needs the generation-time construction event order, including deleted attempts.

### 3.4 Cancel persists cursors and writes only the preview product

Cancel command `0x5C0` writes dialog result `2`. `RandomMapSetupDialog__Run @ 0x00595BC0`
then, regardless of result, writes `DAT_00ABE154` to `RandMap.img` when the wrapper and inner
surface exist, destroys the preview wrapper and `DAT_00ABE150`, and returns `2`.

`ChooseMap__AcceptRandomMapSetup @ 0x005E8590` accepts exactly result `1`; any other result
returns `-1` before the `.SED`, chooser-preview, or sentinel path. Neither teardown nor the parent
cancel path reseeds MapGen, Scenario, or Main. The advanced process cursors therefore survive and
affect later shell work.

### 3.5 Use Map consumes no extra generation when a preview exists

Command `0x6C5` first synchronizes the controls. In skirmish, the condition is:

```text
if ((DAT_00ABE154 != 0 && *DAT_00ABE154 != 0)
    || (RandomMapGenerator__Generate(1, hDlg), g_IsMapEditor == 0))
    result = 1;
```

The left arm short-circuits. A valid existing preview reaches result `1` without calling the
generator again. If no preview exists, OK performs exactly one preview-mode generation, then
accepts in skirmish. The map-editor-only `Save_Scenario_Map_File` branch is not active for the
offline target.

After result `1`, setup teardown writes `RandMap.img`. `0x005E8590` then sets
`DAT_008316D4=1`, writes `MapSeedClass` to `RandMap.Sed` through `0x00597730`, rebuilds the
chooser preview from `RandMap.img`, update-or-appends one ordinary `RandMap.Sed` scenario record,
and returns its index. The Choose Map callback reselects it and commits through normal Use Map
helper `0x005E7160`.

### 3.6 Successful Start replaces the shell Scenario cursor

`Main__PrepareSession @ 0x0052D9A0` calls `Init_Random_Number_System @ 0x0052E619` after a
successful shell return. `Init_Random_Number_System @ 0x0052FC20` seeds a stack Random from the
match seed, copies `0xFD` dwords to `ScenarioClass+0x218`, seeds the stack object again with the
same match seed, and copies `0xFD` dwords to `g_MainRng`. It never touches `g_MapGenRng`.

The ordinary skirmish path then calls `ScenarioClass__Start_Scenario @ 0x0052E745`, which reaches
`ScenarioClass__Read_Scenario @ 0x00684620`. Thus every preview-time Scenario constructor draw is
overwritten before gameplay scenario construction.

### 3.7 `.SED` launch always regenerates

`ScenarioClass__Read_Scenario` tests the final suffix against `.SED` and sets
`ScenarioClass+0x34BD`. On the random branch it:

1. calls `MapSeedClass` reader `0x00597A10(local_filename)`;
2. only on read success calls `RandomMapGenerator__Generate(0,0)` at `0x00684989`;
3. calls `ScenarioClass__Post_Map_Init(1)`;
4. restores the original `.SED` filename into `ScenarioClass+0x125C`.

There is no path from the retained dialog preview object to this branch and no “already generated”
test. MapGen is reseeded/replaced again from the loaded `MapSeed+0x74`; the gameplay map is rebuilt
in memory. During that rebuild, all Techno construction—including CABHUT and failed building
attempts—draws from the match-seeded Scenario RNG after the generator's nested Full-Init prefix.

## 4. Same-Process Retail Trace

The active retail process was `gamemd.exe`, PID `9452`, image base `0x00400000`, started
`2026-08-25 22:40:05 +02:00`. Reads used `ReadProcessMemory`; no debugger writes or binary
annotations were performed.

| Checkpoint | MapGen indices / hash | Scenario indices / hash | File result |
|---|---|---|---|
| Main menu before entering RMG | `0/103`, `BB47A3AA…` | `12/115`, `2CD1A5FB…` | pre-existing `.SED` unchanged |
| First RMG entry | unchanged | `13/116`, `EA585E…` | no write |
| First Generate completes | `139/242`, `04338A3F…` | `33/136`, `8EB5BC…` | `.SED` unchanged |
| Cancel after first preview | unchanged | unchanged | `RandMap.img` written; `.SED` unchanged |
| Re-enter RMG | unchanged | unchanged | no second entry-seed draw |
| Second Generate completes | `33/136`, `5E8EE888…` | `57/160`, `5815F4C5…` | `.SED` still unchanged; new `.img` not yet serialized while setup remains open |
| Later read-only recapture | same `5E8EE888…` | same `5815F4C5…` | no spontaneous cursor movement |

The first entry consumed exactly one Scenario raw word, matching the `Seed == -1` branch. The first
preview then consumed 20 additional raw words and the second preview 24. These counts are not
claimed as CABHUT counts. A live `g_BuildingClass_Array @ 0x00A8EB44`, count
`0x00A8EB50`, census after the second preview found only one surviving object, type `CATHOSP`.
Direct constructor evidence explains the difference: constructor calls consume before later
placement failure/destruction, and multiple RMG building owners share the stream.

The MapGen object's first four bytes also demonstrate whole-object replacement. First Generate
left zero padding; second Generate began `00 63 1A 00` before indices `33/136`. The padding is
stack residue copied by `REP MOVSD`, while the locked byte, indices, and state words remain the
logical RNG state.

File evidence after first-preview Cancel and during the second open setup:

| File | Length | UTC mtime | SHA-256 | Meaning |
|---|---:|---|---|---|
| `RandMap.Sed` | 270 | `2026-07-28 19:59:15.4759372Z` | `B6C9D1A16642459ADC28CF267D2346F08BF931F6F624C0157A546B2F7C5539BB` | pre-existing file remained byte-identical through both previews and Cancel |
| `RandMap.img` | 21,367 | `2026-08-28 16:07:39.3863502Z` | `62359B62557994A0AC63793353DB3820E9156302237A9999DA1DBC3C0EB34CC3` | first generated preview serialized by Cancel teardown |

The `.SED` was backed up byte-for-byte before the trace. No accepted setup write was needed to
prove its order or content gate: those are direct branches in `0x00595BC0` and `0x005E8590`.

## 5. OpenTS Correspondence (Navigation Only)

OpenTS was used only to locate the inherited random-map dialog/control family and the idea of a
seed-options handoff. Its control constants and generator organization were treated as leads.
The YR button ids, active caller chain, RNG instances, `.SED` suffix branch, full-object copies,
CABHUT constructor draw, and retail file behavior above were all independently verified in active
`gamemd.exe` or YR retail data. No OpenTS behavior is parity authority here.

## 6. Current Rust Status and Exact Mismatch

| Rust surface | Current behavior | Native mismatch |
|---|---|---|
| `src/app/frontend/skirmish_session.rs::OfflineSkirmishRuntime` | owns the process shell `scenario_rng` and correctly returns gameplay cursor after a normal match | random-map worker never borrows/transfers this cursor, so preview constructor events cannot advance it |
| `src/app/shell_random_map.rs::start_random_map_generation` | worker calls pure `generate_map_observed` with only `RmgRng`; retains the generated `MapFile` and MapGen continuation | native preview generation also executes the shared Scenario construction sequence |
| `RandomMapGenerationRetention` | moves accepted preview `GeneratedMap` directly into loading | native persists seed/options and regenerates unconditionally from `.SED` after Start reseeds Scenario/Main |
| `src/app/loading/init.rs::retained_random_map_initial` | explicitly claims no second generator is invoked and installs preview output/continuation | claim is contradicted by `0x00684620 -> 0x00597A10 -> 0x00598960(0,0)` |
| ordinary `.SED` branch in `src/app/loading/init.rs` | regenerates from the seed file and carries MapGen continuation | closer to native, but bypassed by the retained-preview shortcut after UI acceptance |
| `LoadingRequest::prepare_battle_start_plan` plus `load_map_from_initial` | prepares a second seed-built Scenario cursor, then constructs another `ScenarioBootstrapRng` later | there must be one match-seeded owner; a plan may retain outcomes but must not manufacture or replace its cursor |
| `src/map/rmg/build.rs` / `pipeline.rs` | generator owns only MapGen RNG and emits successful terrain/structure records | cannot represent Scenario constructor draws, failed object attempts, CABHUT init words, or their order |
| `src/map/rmg/phases/tech_buildings.rs` | records only successful placements | native consumes one Scenario raw word when the building is constructed before up to 100 attempts, even when later deleted |
| `src/map/rmg/phases/carve_driver.rs` | water-class low-bridge branch returns without work and claims it consumes no random draws | active `.SED` maps of types 3/4 call `PlaceLowBridgeDeck`; it consumes MapGen seed-cell/end-coin draws and creates CABHUTs that consume Scenario RNG |
| `src/rng_continuation.rs::MapGenRngContinuation` | transports words and indices from generated map to simulation | transport shape is adequate for logical MapGen state, but it cannot substitute for launch regeneration or the Scenario construction sequence |

The smallest architecture-correct requirement is not “add a Scenario continuation next to
MapGen.” Preview and launch have different Scenario origins: preview continues the shell cursor;
launch starts from the successful-Start match seed. The generator must expose/execute ordered
construction events against the appropriate owner in each run. Reusing the preview continuation
in gameplay would be as wrong as ignoring the draws.

## 7. Coverage Ledger

| Mechanism / branch | Status | Evidence | Remaining |
|---|---|---|---|
| setup control mapping | verified | `0x00596300` command switch | none |
| first-entry seed draw | verified static + runtime | `0x00596BB1..0x00596BC8`; `12/115 -> 13/116` | none |
| repeated-entry no draw | verified static + runtime | seed `!= -1`; unchanged second re-entry cursor | none |
| option Randomize stream | verified | `ECX=0x00886B88` at all command/derived sites | none |
| per-call MapGen reseed/replacement | verified static + runtime | `0x0059897B..0x0059899B`; distinct hashes/padding | none |
| bridge-hut Scenario draw | verified | `0x005904B0 -> 0x0043B740 -> 0x006F3254` | none |
| failed-constructor draw persistence | verified | constructor precedes placement loops/deletion in RMG building owners | none |
| preview Scenario cursor advance | verified runtime | `13/116 -> 33/136 -> 57/160` | none |
| Cancel cursor persistence | verified runtime | hashes unchanged after Cancel/re-entry | none |
| Cancel `RandMap.img` write | verified static + retail file | `0x00595BC0`; current file hash/mtime | none |
| Cancel `.SED` no-write | verified static + retail file | `0x005E8590` result gate; byte-identical file | none |
| OK-with-preview no regeneration | verified | `0x00596300` short-circuit at command `0x6C5` | none |
| accepted `.SED`/preview/sentinel order | verified | `0x00595BC0`, `0x005E8590`, `0x005E7160` | none |
| successful-Start Scenario/Main reseed | verified | `0x0052E619 -> 0x0052FC20` | none |
| `.SED` launch regeneration | verified | `0x0052E745 -> 0x00683AB0 -> 0x00684620 -> 0x00684989` | none |
| preview Scenario isolation from gameplay | verified | Start reseed occurs before `Start_Scenario` | none |
| current Rust delta | verified | direct source scan named in Section 6 | implementation required |

## 8. Open Questions — Final State

- `[RESOLVED] OQ-01 — Which setup button is Use Map? -> decimal 1733 / 0x6C5.`
- `[RESOLVED] OQ-02 — Which stream supplies the initial missing seed? -> Scenario+0x218, exactly one inclusive (0,0xFFFF) call when Seed == -1.`
- `[RESOLVED] OQ-03 — Does reopening draw another seed? -> No while the global MapSeed seed remains non-negative.`
- `[RESOLVED] OQ-04 — Which stream drives Randomize and derived fields? -> g_MainRng @ 0x00886B88.`
- `[RESOLVED] OQ-05 — Does Generate continue MapGen? -> No; every call seeds a stack object from MapSeed+0x74 and replaces all 0xFD dwords of g_MapGenRng.`
- `[RESOLVED] OQ-06 — Are copied padding bytes logical RNG state? -> No; bytes +1..+3 can contain stack residue. Indices and 250 words are the logical cursor.`
- `[RESOLVED] OQ-07 — Can preview generation advance Scenario? -> Yes, through object constructors even though terrain-choice draws use MapGen.`
- `[RESOLVED] OQ-08 — What does each CABHUT allocation consume? -> one raw Scenario Random__Next stored as u16 at Techno+0x3C8.`
- `[RESOLVED] OQ-09 — Is the Scenario delta equal to surviving CABHUTs? -> No; all RMG Techno constructors share the stream and failed placements already consumed.`
- `[RESOLVED] OQ-10 — Does Cancel restore either RNG? -> No; both advanced objects persist.`
- `[RESOLVED] OQ-11 — Does Cancel write RandMap.img? -> Yes when a generated preview surface exists.`
- `[RESOLVED] OQ-12 — Does Cancel write RandMap.Sed or commit the sentinel? -> No; 0x005E8590 returns -1 for result 2 before those side effects.`
- `[RESOLVED] OQ-13 — Does Use Map regenerate an existing preview? -> No; the preview pointer short-circuits the generator arm.`
- `[RESOLVED] OQ-14 — What if Use Map is pressed without a preview? -> skirmish performs one preview-mode generation and then returns accepted result 1.`
- `[RESOLVED] OQ-15 — When is RandMap.Sed written? -> after accepted setup teardown and before sentinel upsert/ordinary chooser commit.`
- `[RESOLVED] OQ-16 — Does successful Start keep the shell Scenario cursor? -> No; Init_Random_Number_System replaces Scenario and Main from the match seed.`
- `[RESOLVED] OQ-17 — Does `.SED` launch reuse the preview map? -> No; Read_Scenario loads options and unconditionally calls generator (0,0) on reader success.`
- `[RESOLVED] OQ-18 — Does launch MapGen continue the preview cursor? -> No; the generator reseeds/replaces it from the loaded seed again.`
- `[RESOLVED] OQ-19 — Do preview constructor draws affect gameplay Scenario order? -> No; the Start reseed erases them. Equivalent launch-time constructor draws do affect gameplay.`
- `[RESOLVED] OQ-20 — Is current Rust retained-map handoff native? -> No; it contradicts the verified second-generation branch and omits the generation-time Scenario sequence.`
- `[RESOLVED] OQ-21 — Can the fix carry the preview Scenario continuation into gameplay? -> No; preview and gameplay have different seed origins.`
- `[RESOLVED] OQ-22 — Is OpenTS evidence sufficient for any conclusion above? -> No; it was navigation only and every material claim was independently proved in YR.`

## 9. Visual / UI State Ledger

This slice does not claim pixel-exact dialog rendering. It records only UI-visible states that
activate or expose the RNG/file lifecycle.

| Order | State / function | Control / asset | Visible role | Evidence |
|---:|---|---|---|---|
| 1 | setup init `0x00596300` | Generate `0x620`; Use Map `0x6C5` disabled | no accepted preview yet | decompile |
| 2 | Generate | working label `0x638`; all 13 controls disabled | generation/progress state | decompile |
| 3 | `GenerateTerrainPreview @ 0x00641140` | `DAT_00ABE154` surface | generated map thumbnail in setup | decompile + live dialog |
| 4 | Generate returns | Use Map/Save enabled | preview may now be accepted | decompile + live accessibility tree |
| 5a | Cancel | `RandMap.img` file written, chooser shown again | previous committed chooser selection remains | decompile + retail file trace |
| 5b | Use Map | no extra generation when preview valid | setup closes as result 1 | decompile |
| 6 | accepted caller | `RandMap.Sed`, `RandMap.img`, sentinel row | chooser commits ordinary `RandMap.Sed` selection | decompile |
| 7 | Start | loading screen, in-memory regenerated scenario | preview image is not gameplay terrain | `0x0052E619`, `0x00684620` |

Asset role matrix:

| Asset/state | Setup preview | Accept handoff | Gameplay source | Persistent RNG authority |
|---|---|---|---|---|
| `RandMap.img` | yes | chooser preview | no | none |
| `RandMap.Sed` | no write until accept | seed/options + selected identity | yes, reader input | none |
| preview-generated `MapClass` | temporary process state | not handed through `.SED` branch | no | none |
| `g_MapGenRng` | replaced/advanced | remains in process until next generation | replaced/advanced again | logical state after launch generation |
| shell Scenario RNG | constructor side effects | persists until Start | overwritten before scenario load | no |
| gameplay Scenario RNG | not yet established | not yet established | construction/gameplay stream | yes after successful Start |

## 10. Implementation Handoff

| Verified requirement | Evidence | Current Rust delta | Required effect | Acceptance scenario | Do not do |
|---|---|---|---|---|---|
| Every RMG call starts MapGen from current seed. | `0x0059897B..0x0059899B`; live hashes | preview retention is treated as launch authority | Run launch RMG from `.SED`; a repeated call begins from seed, not prior continuation. | Generate preview, accept, launch: generator start cursor is `0/103`; final MapGen continuation matches a fresh same-seed run. | Do not feed preview continuation into a second generator or skip launch generation. |
| Preview construction advances the process shell Scenario stream. | `0x006F3249..0x006F3259`; retail cursor trace | worker has no Scenario owner | Move/borrow the front-end Scenario cursor into preview generation and return it with ordered constructor effects, including failed attempts. | Two previews advance the shell cursor by their exact construction-event counts; Cancel/reopen continues it. | Do not infer draws from surviving emitted structures. |
| CABHUT allocation is one ordered Scenario raw draw. | `0x005904B0 -> 0x0043B740 -> 0x006F3254` | low-bridge branch and huts absent | Emit/instantiate each hut at its native point in the RMG phase and consume/store the corresponding Scenario word. | Fixed map/Scenario seeds produce identical hut list, `+0x3C8` values, and post-generation cursor. | Do not create huts after all other structures or use MapGen for their init word. |
| Cancel keeps advanced RNGs, writes temp image, and does not commit `.SED`. | `0x00595BC0`, `0x005E8590`; retail files | candidate cancellation drops pure generated data but has no Scenario result to retain | Preserve returned shell Scenario cursor; discard candidate/accepted map; keep `.SED`/selection unchanged; write the preview product when one exists. | Generate then Cancel: Scenario differs from pre-generate, `.img` changes, `.SED` and committed map do not. | Do not roll the shell RNG back transactionally. |
| Successful Start reseeds Scenario/Main before `.SED` generation. | `0x0052E619` before `0x0052E745`; nested `0x00599650 -> 0x00686B20` | retained preview bypasses launch generator and loading constructs parallel seed-derived cursors | Create one gameplay `ScenarioBootstrapRng`; carry it through the Full-Init house/start/Fill prefix, ordered RMG construction events, Post-Map start work, projection, and final Simulation handoff. | Preview draw count has no effect on gameplay; the post-launch cursor matches the complete native nested order. | Do not carry shell Scenario state into match or start a second match-seeded cursor for preload. |
| `.SED` reader success gates a new generator `(0,0)`. | `0x00684620`, `0x00684975..0x00684990` | UI accepted path uses `retained_random_map_initial` | Remove or bypass retained-map authority; regenerate from persisted options and retain `.SED` identity. | Accepted UI map and a fresh process launching the same `.SED` produce the same map, construction sequence, and RNG cursors. | Do not use `RandMap.img` or cached preview `MapFile` as gameplay terrain. |

Suggested proof tests:

- `rmg_preview_reseeds_mapgen_but_advances_shell_scenario_constructor_sequence`
- `rmg_preview_cancel_keeps_scenario_cursor_and_does_not_write_sed`
- `rmg_accept_with_existing_preview_does_not_run_third_generation`
- `rmg_launch_reseeds_gameplay_scenario_before_sed_regeneration`
- `rmg_launch_does_not_retain_preview_generated_map`
- `rmg_cabhut_constructor_consumes_one_shared_scenario_word_in_generation_order`
- `rmg_failed_building_attempts_still_advance_scenario_constructor_cursor`
- `rmg_first_setup_entry_draws_seed_once_but_reentry_does_not`
- `rmg_use_map_without_preview_runs_exactly_one_generation`
- `rmg_generated_low_deck_projection_does_not_replay_overlay_mark`
- `authored_techno_constructors_consume_scenario_in_native_section_and_upgrade_order`
- `generated_techno_projection_consumes_no_second_constructor_draw`

## 11. Negative Facts / Evidence-Backed Exclusions

- Do not treat `g_MapGenRng` as process-continuing across Generate calls. It is overwritten by a
  seed-built `0xFD`-dword object at every entry.
- Do not use Scenario RNG for RMG terrain-choice draws or Main RNG for bridge placement. Terrain
  and bridge selection use MapGen; Scenario is consumed by constructed Technos; option Randomize
  uses Main.
- Do not equate the Scenario delta with surviving structures or CABHUT count. Construction can
  precede failure and destruction.
- Do not roll RNG state back on setup Cancel. Native only suppresses `.SED`/sentinel/selection
  commit.
- Do not claim setup Cancel has no filesystem side effect. A generated preview is written as
  `RandMap.img` during common teardown.
- Do not generate again on Use Map when a valid preview exists. The valid surface short-circuits
  the generator arm.
- Do not retain the preview `MapClass` as the gameplay scenario. `.SED` load calls the generator
  again unconditionally on reader success.
- Do not carry preview Scenario state into gameplay. Successful Start replaces Scenario/Main first.
- Do not model CABHUT's constructor word with a bridge-private RNG. It is one shared Scenario stream
  with all other Techno construction and later gameplay consumers.
- Do not use OpenTS control ids, state lifetimes, or generator retention as YR authority.
- Do not add `0x005A6510`, `0x005A82E0`, or `0x005A91E0` phases to the active RMG trace. Their
  RMG-shaped code has no active generator caller or stored function pointer in this executable.
- Do not postpone authored-map Techno constructor draws until after fixed-map projection. Native
  consumes at construction, before Unlimbo, in section/key order; later placement failure does not
  roll the word back.

## 12. Design-Review Follow-up: One Launch Cursor and No Overlay Replay

### 12.1 Native launch nesting fixes the cursor order

The launch generator is not a pure call made against an otherwise untouched Scenario cursor.
`RandomMapGenerator__Generate(0,0)` calls
`RandomMapGenerator__InitMapFromSyntheticINI @ 0x00599650`. Its `preview == 0` branch calls
`ScenarioClass__Full_Init @ 0x00686B20` before any water, connector, bridge, CABHUT,
starting-point, or neutral-tech phase. `Full_Init` creates Houses, runs the selected-mode start
callbacks, and reads the synthetic map/Fill on the same process Scenario object. Only after that
call returns does the generator reach, in order:

1. type-3/type-4 bridge and connector work, including CABHUT constructors;
2. cell recalculation and generated starting points;
3. neutral-tech construction attempts;
4. remaining terrain and resource phases;
5. return to `Read_Scenario`, followed by `Post_Map_Init` start-unit, crate, and later house work.

The native owner is one match-seeded Scenario object spanning this nested call graph. A Rust port
may precompute MapGen-only geometry earlier, because the constructor word does not choose whether
an RMG placement succeeds, but it must replay every ordered construction attempt at the native
point in that one cursor. It may not calculate the battle plan with a throwaway `SimRng`, start a
second cursor for Fill, or replay constructors against a third cursor.

For current Rust, `ScenarioBootstrapRng` is the correct owner but its lifetime begins too late.
`LoadingRequest` must create and own it once from the launch seed. Battle-start preloading consumes
that owner directly and retains resolved outcomes rather than a replacement cursor. The same owner
moves into `load_map_from_initial`; terrain Fill continues the Full-Init prefix, then the ordered
RMG construction trace advances it. `into_simulation` finally transfers that same cursor so
Post-Map and gameplay consumers continue it.

### 12.2 Required construction trace and binding contract

Precomputed RMG geometry must return a stable ordered `RmgConstructionTrace`; it cannot return
only surviving `MapEntity` rows. Each event identifies its ordinal, construction phase
(`BridgeRepairHut` or `NeutralTech`), type identity, and outcome. The outcome is either
`Discarded` or `Emitted { entity_index, cell }`. A discarded neutral-Techno event has no required
cell: both active neutral-tech owners construct before their placement-attempt loops, and a fully
failed object has no native final construction cell. Inventing one would create a false validation
key. CABHUT construction occurs after a qualifying candidate cell is found, but its discarded
candidate still needs no projection binding.

Launch replay consumes exactly one raw Scenario draw for every event. A discarded event consumes
and drops the low word. An emitted event consumes once and records that low word in a
`GeneratedTechnoInitTable` keyed by the stable generated entity index. `MapLoadInitial` carries
the trace into launch and the completed binding table into projection.

`spawn_from_map_with_resolved` must accept the optional generated-init table, validate the emitted
entity index, type, and cell identity, and install the precomputed `techno_ctor_random_word` on the
spawned `GameEntity` without another draw. That field belongs in deterministic snapshots/hashes.
Authored-map Technos continue through the ordinary constructor-draw path; only a validated RMG
binding suppresses that draw. This makes both rules explicit: failed generated attempts consume
without a binding, while each successful generated entity consumes once and is never double-drawn.

Preview generation returns the same ordered trace to the shell. The shell leases its existing
front-end Scenario cursor, replays every event once, and keeps the resulting cursor even if the
candidate is cancelled. Preview bindings are display-only and never become launch input.

### 12.3 Generated low decks are already materialized

`RandomMapGenerator__PlaceLowBridgeDeck @ 0x0058F2C0` directly writes every cell in the complete
three-wide overlay rectangle. East-west endpoints are `0x5E` and `0x5C`, east-west body cells are
`0x4A + (x % 4)`, north-south endpoints are `0x60` and `0x62`, north-south body cells are
`0x53 + (y % 4)`, and the cross-section index is written to cell `+0x11E` for all three rows. The
routine does not call `OverlayClass::Mark` and consumes no Scenario RNG.

The successful `.SED` branch in `ScenarioClass__Read_Scenario @ 0x00684620` is exclusive with the
ordinary scenario-INI reader and does not later call `ReadMapOverlayPacks`. Therefore the launch
generator's low-deck rectangle is already the final materialized payload. Rust must tag the
generated source explicitly and skip fixed-map procedural `OverlayClass::Mark` expansion for it.
Fixed authored maps retain their ordinary endpoint-driven Mark path. Replaying Mark over generated
deck cells would change variants and consume Scenario draws that native launch never makes.

`0x005A6C10` is the direct RMG isometric tile/subtile/slope stamper and also consumes no RNG.
The stale `0x00578E60` label is a cliff-level/face fixup, not a low-bridge Mark routine, and is
excluded as authority for generated bridge overlays.

### 12.4 Added acceptance gates

- First RMG setup entry with `Seed == -1` consumes exactly one shell Scenario seed draw; reopening
  with the now-valid global seed consumes zero seed draws.
- Use Map without an existing preview runs exactly one preview-mode generation before acceptance.
- One launch `ScenarioBootstrapRng` advances through battle preload, Fill, every RMG construction
  event, projection, Post-Map work, and final Simulation ownership without reseeding or cursor
  replacement.
- A failed RMG construction event advances the cursor but creates no binding; an emitted event
  advances once, binds its word, and projection performs no second draw.
- Generated low-deck projection preserves all direct overlay ids/cross indices and performs no
  fixed-map Mark replay or Scenario draw.

### 12.5 Authored-map Techno construction is a required cursor prerequisite

The constructor word is not RMG-specific. `TechnoClass__Constructor @ 0x006F2B90` ends with the
unconditional raw Scenario `Random__Next @ 0x006F3254`, storing the low word at `Techno+0x3C8`.
`BuildingClass__Constructor @ 0x0043B740` calls it directly;
`UnitClass__Constructor @ 0x007353C0`, `AircraftClass__Constructor @ 0x00413D20`, and
`InfantryClass__Constructor @ 0x00517A50` reach it through
`FootClass__Constructor @ 0x004D31E0`.

For a fixed authored scenario, `ScenarioClass__Full_Init @ 0x00686B20` constructs Technos in
hard-coded section order after terrain:

1. `[Units]` through `ScenarioClass__Read_Units_Section @ 0x00743270`;
2. `[Aircraft]` through `0x0041B110`;
3. `[Infantry]` through `0x0051FB00`;
4. `[Structures]` through `BuildingClass__ReadFromINI @ 0x0044F820`.

Every reader walks INI keys upward from index zero. A valid house/type and successful allocation
reach the derived constructor before Unlimbo, so an object that later fails Unlimbo has already
consumed its one word. Malformed rows, unknown houses/types, and allocation failure do not reach
the constructor and consume none.

The Structures reader can also construct authored upgrades after a base building successfully
Unlimbos. It reads the upgrade count and three type slots, then the loop at
`0x0044FD50..0x0044FDC3` constructs each selected non-`-1` upgrade in slot order through the same
Building/Techno constructor chain. Those side constructors therefore also consume one word each;
they cannot be omitted from the cursor contract merely because current Rust `MapEntity` does not
yet retain the upgrade payload.

Current Rust already parses base `MapEntity` rows in native category order, and
`construct_scenario` projects them in that slice order, but `spawn_from_map_with_resolved`
currently spends no Scenario draw and `GameEntity` has no constructor-word field. Exact bridge RNG
continuation therefore requires a small shared load prerequisite before the RMG unit:

- keep one constructor cursor owner through the existing section-order projection;
- consume/store one raw word for every native-valid base or upgrade constructor, including a
  later failed placement;
- give a fixed authored entity its directly consumed word;
- give a generated emitted entity its validated preconsumed binding and draw zero at projection;
- consume no word for a discarded generated event at projection, because it was already consumed
  during trace replay.

The production oracle is an interleaved fixed-map fixture with one valid row in each Techno section,
one later-failed Unlimbo case, and a structure upgrade. It asserts word-to-object assignment and the
post-load Scenario cursor in `[Units]`, `[Aircraft]`, `[Infantry]`, `[Structures]`, upgrade-slot
order. A paired generated fixture asserts that projection leaves the cursor unchanged.

### 12.6 The constructor invariant continues through Post-Map and runtime creation

The authored-map prerequisite is not the end of the shared cursor obligation.
`ScenarioClass__Post_Map_Init @ 0x00686890` invokes
`MultiplayerGameMode__Generate_Starting_Units @ 0x005D6D80` after scenario load. Its game-mode
callback at vtable `+0xC8` is
`MultiplayerGameMode__Create_Starting_Base_Unit @ 0x005D7030`: the active Bases path allocates a
`UnitClass`, calls `UnitClass__Constructor @ 0x007353C0` from `0x005D7079`, then tries exact Unlimbo
and the radius-one fallback. Total placement failure deletes the already-constructed object, so its
unconditional Techno constructor word remains spent.

The companion game-mode callback at vtable `+0xCC` is the active routine at `0x005D70F0`. It builds
eligible UnitType/InfantryType candidate arrays, selects candidates with the same Scenario owner,
and calls the chosen type's `CreateObject` virtual at `0x005D7393` before placement. The concrete
factories are `UnitTypeClass` vtable `+0x8C -> 0x00747560` and
`InfantryTypeClass__CreateObject @ 0x00523B10`; they allocate and call
`UnitClass__Constructor @ 0x007353C0` or `InfantryClass__Constructor @ 0x00517A50`, respectively.
Those paths therefore reach the same unconditional base draw before their later placement result.

The decisive boundary is class-level rather than a list of favored callers: every active derived
Techno construction reaches `TechnoClass__Constructor @ 0x006F2B90`, whose final raw Scenario draw
and `Techno+0x3C8` store are unconditional. Ordinary production, free units, sell survivors,
Post-Map starting forces, paradrop cargo, spawn-manager children and other runtime constructors must
therefore use the same fresh-word path. A later Unlimbo/placement failure does not refund the draw.
Snapshot restore is not construction and must restore the serialized word without drawing again.

The stored word is persistent gameplay state, not disposable cursor padding. Two independent active
readers use it to select a report sound:

- `DiskLaserClass__AI @ 0x004A7340`, at `0x004A76CC`, loads the source
  `Techno+0x3C8`, divides by `WeaponType+0xCC` report count, and uses the remainder to select the
  report entry before `VocClass__PlayAt`;
- `TechnoClassFireAtSpawnsBullet @ 0x006FDD50`, at `0x006FF36B`, performs the same unsigned-word
  modulo selection for an ordinary firing Techno's report list.

An authored structure upgrade is itself a live `BuildingClass`/`TechnoClass` object, so its word
cannot be represented by a cursor-only discard or by an ephemeral parser side effect. The minimum
faithful Rust owner is a distinct `GameEntity` carrying `techno_ctor_random_word` plus a stable
`StructureUpgradeLink { parent_stable_id, slot }`. The link does not claim that the broader upgrade
behavior backlog is closed; it gives the native constructor object a persistent deterministic
identity and preserves the word for later Techno consumers, snapshots, and hashes.

The current Rust construction census is small enough to enforce one funnel. Outside tests,
`GameEntity::new_at_frame` is called only at the three construction sites in
`src/sim/world/world_spawn.rs`: authored/generated map projection,
`spawn_object_at_height`, and `spawn_object_limbo_at_height`. `spawn_object` delegates to the
height-aware path. Active scenario-bootstrap, production, sell-survivor, refinery free-unit,
slave-miner, spawn-manager and superweapon paths all route through those runtime entry points.
Snapshot load restores serialized `GameEntity` state and is a non-drawing path.

The implementation handoff must therefore expose one internal constructor initialization mode and
assign the field exactly once:

- `FreshScenario`: draw/store before any Unlimbo or placement decision; use for fixed authored map,
  Post-Map and runtime construction;
- `PreconsumedGenerated`: validate the RMG stable-index/type/cell binding, install its word and draw
  zero;
- `Restored`: reinstate the serialized word under the existing native-verified restore cursor
  contract, drawing zero.

Direct production use of `GameEntity::new_at_frame` outside that funnel must become impossible or
be guarded by a source-boundary test. In addition to the fixed-map and generated fixtures above,
acceptance requires (1) Post-Map starting MCV and extra-unit word/cursor assertions, including a
later placement failure, (2) an ordinary runtime production/spawn assertion, and (3) a snapshot
round trip proving the installed word persists without an extra draw.

## 13. Sources

- Fresh read-only Ghidra decompile: `0x00596300`, `0x00595BC0`, `0x005E8590`,
  `0x005981F0`, `0x005904B0`, `0x00595400`, `0x005A6510`, `0x005A82E0`,
  `0x005A91E0`, `0x0052FC20`, `0x00684620`, `0x00641140`.
- Fresh read-only Ghidra assembly/search/caller census: `0x00596768..0x00596845`,
  `0x00596BA0..0x00596BD5`, `0x00598960..0x00598A20`,
  `0x00598CB0..0x00598DD0`, `0x006F3230..0x006F3280`,
  `0x0052E5C0..0x0052E770`; callers of `0x0065C780` and `0x0043B740`.
- Same-process retail `ReadProcessMemory` snapshots of `0x00ABE890`, `0x00A8B230`,
  `Scenario+0x218`, `0x00A8EB44`, and `0x00A8EB50`; retail file hashes/metadata for
  `RandMap.Sed` and `RandMap.img`.
- Reconciled reports:
  `MAPGEN_SAME_PROCESS_LIFECYCLE_BRIDGE_CALLER_RECONCILIATION_GHIDRA_REPORT.md`,
  `RMG_BRIDGE_CONNECTOR_PASS_0058EF10_GHIDRA_REPORT.md`,
  `RMG_RNG_SEED_MAPGENRNG_GHIDRA_REPORT.md`,
  [RMG_WATERAMOUNT_DERIVATION_00597260_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/RMG_WATERAMOUNT_DERIVATION_00597260_GHIDRA_REPORT.md),
  [SKIRMISH_CREATE_RANDOM_MAP_0X583_ACCEPT_CANCEL_STATE_MACHINE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CREATE_RANDOM_MAP_0X583_ACCEPT_CANCEL_STATE_MACHINE_GHIDRA_REPORT.md),
  [SKIRMISH_CREATE_RANDOM_MAP_0X583_BROAD_RECHECK_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_CREATE_RANDOM_MAP_0X583_BROAD_RECHECK_GHIDRA_REPORT.md), and
  [SKIRMISH_RANDOM_COLOR_AND_SETTINGS_PERSISTENCE_TRIGGER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/SKIRMISH_RANDOM_COLOR_AND_SETTINGS_PERSISTENCE_TRIGGER_GHIDRA_REPORT.md).
- Design-review follow-up decompile/call-order census: `0x00599650`, `0x00686B20`,
  `0x0058F2C0`, `0x005A6C10`, and `0x00686890`; reconciled against
  [SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/skirmish-ui/SKIRMISH_START_TO_FULL_INIT_SPAWN_TRACE.md).
- Revision-5 follow-up: caller/xref and pointer-byte census for `0x005A5020`, `0x005A6510`,
  `0x005A82E0`, and `0x005A91E0`; authored constructor/loader decompiles
  `0x006F2B90`, `0x004D31E0`, `0x00743270`, `0x0041B110`, `0x0051FB00`, and
  `0x0044F820`; structure-upgrade assembly `0x0044FD50..0x0044FDC3`; reconciled against
  [ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ACTIVE_OBJECT_ORDER_SOURCE_LOAD_REVEAL_SPAWN_GHIDRA_REPORT.md).
- Revision-6 follow-up: `ScenarioClass__Post_Map_Init @ 0x00686890`,
  `MultiplayerGameMode__Generate_Starting_Units @ 0x005D6D80`, starting-base callback
  `0x005D7030`, active extra-unit callback assembly `0x005D70F0..0x005D7498`, concrete
  Unit/Infantry type factories `0x00747560` and `0x00523B10`, plus persistent constructor-word
  readers `0x004A76CC` and `0x006FF36B`.
- Current Rust: `src/app/frontend/skirmish_session.rs`, `src/app/shell_random_map.rs`,
  `src/app/loading/init.rs`, `src/map/rmg/build.rs`, `src/map/rmg/pipeline.rs`,
  `src/map/rmg/phases/tech_buildings.rs`, `src/map/rmg/phases/carve_driver.rs`,
  `src/map/entities.rs`, `src/sim/world/world_spawn.rs`, `src/sim/scenario_bootstrap.rs`, and
  `src/rng_continuation.rs`.
- OpenTS: readable secondary navigation reference only; no material conclusion above relies on it.
