# Ground Movement Executable Native Oracle Capture — Preflight and Blocker Report

**Date:** 2026-07-22  
**Address(es):** no fresh decompilation; inherited static anchors are enumerated in Section 9 and require hook-enrollment revalidation  
**Investigation Mode:** coverage-map  
**Checkpoint:** E of `2026-07-20-ground-movement-atomic-flip-readiness-investigation-plan.md`  
**Claimed Scope:** read-only feasibility audit, capture-method selection, exact capture envelope, fixture matrix, tooling readiness, and the work required before retail movement captures can be made  
**Non-Scope:** launching or controlling `gamemd.exe`; native input; debugger attach; injection; hook enrollment; evidence mutation; private-oracle source changes; Rust gameplay changes; Cargo; Ghidra mutation; production activation  
**Confidence:** HIGH for the observed tooling state and frozen A–D requirements; no runtime parity confidence is claimed because no Checkpoint-E movement capture exists  
**Active in YR:** Conditional — the probed movement paths are active standard-YR paths established by the cited A–D reports, but this report did not execute them  
**Checkpoint Verdict:** **BLOCKED / PRECHECK_FAILED**  
**Production Verdict:** **NO-GO**

## 1. Executive Verdict

Checkpoint E cannot be executed with the currently reviewed oracle state.

The private oracle repository contains useful foundations: a strict operator facade,
SyringeEx hook manifests, two shared-memory transports, a 3,416-byte offline
`StateSnapshot` codec, debugger collectors, and a native DXGI capture collector.
Those pieces do not currently form a movement oracle:

1. `create-checkpoint` and `run-original` are registered commands but are still
   `STUB`.
2. The finalized MTNK scenario is absent; only an unsealed recipe exists.
3. The active hook manifests contain startup and completed-tick hooks, not movement,
   locomotor, lifecycle, or command-consumption hooks.
4. The instrument does not call the state-snapshot-v2 encoder. No live producer
   publishes the existing snapshot schema.
5. The existing MTNK state block omits essential Checkpoint-E fields: RawTrack
   selector/cursor/short byte, residual, current and target speed fractions, full
   path, LogicVector order, occupancy/lifecycle state, and ordered effect receipts.
6. `oracle.py status --json` reports `parity_authority=NONE`; original-YR execution,
   instrumentation, input authority, and cross-engine comparison are all blocked.
7. `oracle.py doctor` reports `PRECHECK_FAILED` because the local reviewed-tool lock
   is invalid, the native DXGI source identity differs from the enrolled manifest,
   and native capture is consequently unenrolled.
8. Actual execution would cross separately reviewed safety boundaries: game launch,
   native input, injection, filesystem/evidence mutation, or debugger attachment.
   No such approval was granted for this slice.

The selected method is therefore a **two-lane oracle**:

- use bounded SyringeEx instrumentation and a lock-free shared-memory producer for
  atomic mechanism, state, RNG, invocation-order, and same-tick effect records;
- use separate clean, debugger-free and injection-free DXGI runs for wall-clock
  cadence, jitter, and pixel evidence.

Debugger traces remain useful for low-hit calibration and validating proposed hook
addresses. They must not be treated as final pacing evidence, and external
`ReadProcessMemory` samples must not be treated as an atomic completed-tick state
oracle.

No executable retail movement fixture, machine-readable movement capture, or
gamemd-versus-Rust comparison was produced. The investigation plan's stop condition
at lines 388–394 fired: runtime capture requires mutation and tooling authority not
granted to this task.

## 2. Authority, Identity, and Frozen Read Boundary

### 2.1 Retail executable

| Field | Value | Evidence |
|---|---|---|
| Path | `<ra2-install>\gamemd.exe` | direct read-only filesystem inspection |
| Size | `5,286,504` bytes | `Get-Item` |
| SHA-256 | `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C` | `Get-FileHash -Algorithm SHA256` |
| File/product version | `1.11` | executable version resource |
| PE timestamp / image base | `0x3BDF544E` / `0x00400000` | reviewed hook manifests and startup research |
| Target | active retail Yuri's Revenge executable | all cited A–D reports and private hook-manifest identity |

Every future evidence bundle must repeat this identity. A capture against a different
executable is not evidence for this checkpoint.

### 2.2 Public movement-research state

At the original preflight status read, the public repository was at HEAD
`cbf4d8711d6c136964a2e9210c442e1c79542d69` and the only pre-existing dirty path
was `src/sim/world/techno_ai.rs`. At the 2026-07-22 resumption read, HEAD was
`5635788194131976a032803b829b6dba378565c1` and the unrelated dirty paths were
`src/map/rmg/build.rs` and `src/sim/world/techno_ai.rs`. None was read as settled
production authority or modified. Checkpoint-E requirements were derived from the
content-addressed research and contract documents listed in Sources, whose hashes
were rechecked after resumption.

### 2.3 Private oracle state

The private oracle repository was read at HEAD
`7b8689edd2c5a26ec936caaa03d2c7c9bc31523e`. At the original preflight read, nine
pre-existing modified files under `tools/input_provenance_lab/` were active
unrelated work. During review, companion work also modified
`docs/plans/2026-07-20-stage-13b-primary-certification-plan.md`. All ten paths were
excluded as moving worktree authority and were not modified. Where this report
cites the Stage-13B plan, it explicitly cites clean-HEAD blob
`818314c6c8174f4401699bf63226389067a445ee`, not the later companion WIP.

Private `AGENTS.md` permits read-only inspection but requires separate reviewed
approval for launch, native input, injection, debugger attach, enrollment,
replacement, evidence mutation, or cleanup. It also makes
`tools/oracle_harness/oracle.py` the sole supported operator/LLM facade. This report
used only its read-only `capabilities`, `status`, `workspace-status`,
`startup-lifecycle-recipe`, and `doctor` surfaces.

### 2.4 No-mutation statement

This investigation:

- did not start or attach to `gamemd.exe`;
- did not start a debugger backend;
- did not inject a DLL or enroll a hook manifest;
- did not send native input;
- did not create, replace, finalize, restore, or remove oracle evidence;
- did not write under `<local>/Documents/vera20k-oracle`;
- did not edit Rust, run Cargo, stage, commit, or mutate Ghidra.

The only write is this public research report.

## 3. What Checkpoint E Must Prove

Checkpoint E is not a screenshot smoke and not a single MTNK final-state sample. It
must provide retail-derived, replayable evidence for the intermediate state and
ordering comparisons required by the approved movement design and the A–D contract.

The minimum ten Phase-5 scenarios at plan lines 270–280 are a smoke set. The exit
criterion at lines 286–290 is stronger: capture fields must cover **every acceptance
comparison** in the design and contract. Consequently the detailed branch,
population, lifecycle, and later-object-visible discriminators in Sections 7–8 of
this report are required before a production cutover can be called verified.

No executable capture can by itself prove that every old Rust production caller was
removed. Static source review must separately prove atomic caller retirement,
removal of pass-wide `already_scattered` and contact-derived bulk authority, and the
absence of a handled-ID bridge.

## 4. Selected Oracle Method

### 4.1 Method ranking

| Rank | Method | Perturbation | What it can prove | What it cannot prove | Verdict |
|---:|---|---|---|---|---|
| 1 | SyringeEx callbacks → bounded shared-memory records, plus separate clean DXGI runs | instrumented state lane; clean presentation lane | atomic state/order/RNG/event receipts; clean cadence and pixels | observer non-perturbation until separately certified | selected |
| 2 | GhidraMCP/DbgEng hardware execute breakpoints plus memory reads | stops target on hits | low-hit address/receiver/field calibration | undisturbed cadence; one atomic multi-object/RNG view | diagnostic only |
| 3 | standalone Pybag/DbgEng software traces | exception and resume per hit | rapid prototype traces | hot movement cadence and non-perturbed ordering | calibration only |
| 4 | external DXGI Desktop Duplication alone | low | pixel frames, present timing, shell/game presentation | hidden object state, RNG, LogicVector order, occupancy | clean lane only |
| 5 | isolated Unicorn leaf execution | none to retail process | relocatable pure leaf routines | live object graph, lifecycle, scheduler order | not suitable for E |

### 4.2 Why external memory sampling is insufficient

The existing MTNK snapshot design explicitly rejects external
`ReadProcessMemory` as final state authority: sequential reads cannot prove one
atomic completed-tick view across three RNG objects and the tracked unit without a
stop/suspend mechanism, while a stop mechanism perturbs runtime scheduling. The
same problem becomes larger for Checkpoint E because it needs live-vector order,
multiple movers, occupancy, lifecycle mutation, and later-object visibility.

External reads remain acceptable for calibration when their diagnostic role and
timing disturbance are explicit.

### 4.3 Why cadence must be a separate clean lane

`Main_Tick` and locomotor breakpoints can delay the target. Hook callbacks also add
work. Neither lane may be used to claim native wall-clock throughput or jitter.

For each local GameSpeed value, record clean-run high-resolution timestamps and
DXGI presentation data separately from instrumented state runs. Treat the observed
throughput as a runtime measurement. Do not convert it into a Drive-specific
divisor: the static scheduling report already proves one live-object pass per
reached Main Tick and one eligible locomotor Process opportunity per Foot turn.

### 4.4 Observer constraints

The final instrumented producer must be:

- read-only with respect to game state;
- allocation-free and bounded on the target thread;
- free of target-side file I/O, locks, waits, logging frameworks, and unbounded
  traversal;
- fail-closed on missing identity, capacity, ordering, or mapping authority;
- register/EFLAGS/FPU/SIMD preserving at every installed hook;
- sequence-numbered and capable of reporting overflow without overwriting
  unacknowledged evidence;
- paired with an observer non-perturbation proof before hidden-state evidence can
  certify parity.

Repeatable instrumented output proves repeatability, not non-perturbation.

There is also a known installed-shim blocker, not merely a missing future proof.
The current DLL callback shims preserve FXSAVE state and LastError inside the
callback. Pinned SyringeEx then writes the callback return value to TEB
`FS:[0x14]` **after** callback return, so callback-local restoration cannot undo
that write. Exact installed-observer non-perturbation remains blocked until the
loader side preserves/restores this TEB location or positive native evidence proves
that the write is unconsumed and fully equivalent over the capture scope. The
current report proves neither alternative.

## 5. Current Oracle Tooling State

### 5.1 Facade status

Read-only `oracle.py status --json` returned:

| Item | Observed state | Consequence |
|---|---|---|
| topology | `VALID` | repository registry is internally parseable |
| parity authority | `NONE` | no current pipeline may certify parity |
| `create-checkpoint` | `STUB` | the recipe cannot become a sealed scenario through the supported facade |
| `run-original` | `STUB` | no supported original-YR execution pipeline exists |
| input-provenance lab | `QUALIFICATION`, integration not wired | no production command authority |
| instrumentation pipeline | `BLOCKED` | no supported movement instrumentation run |
| original-YR execution pipeline | `BLOCKED` | no supported retail execution run |
| cross-engine comparison | `BLOCKED` | comparator and VERA adapter anchors absent |

The registry SHA-256 reported by both `status` and `capabilities` was
`04B7AB1E5764B638C038DF3E54BD0C09726816F16388B94369C19CEB03C61EDF`.

### 5.2 Safety classes

`oracle.py capabilities --json` classifies:

| Command | State | Safety class | Reviewed approval |
|---|---|---|---|
| `capabilities`, `status`, `doctor`, `workspace-status`, `startup-lifecycle-recipe` | implemented | `READ_ONLY` | not required |
| `inspect-shell` | implemented | `GAME_LAUNCH` | required |
| `navigate-shell` | implemented | `NATIVE_INPUT` | required |
| `run-original` | stub | `INJECTION` | required |
| `create-checkpoint` | stub | `NATIVE_INPUT` | required |
| enrollment/replacement/finalization commands | implemented | `FILESYSTEM_MUTATION` | required |

No command in the last five rows was executed.

### 5.3 Doctor result

Read-only `oracle.py doctor` at `2026-07-21T23:44:15.277369Z` returned
`PRECHECK_FAILED`:

| Check | Status | Exact observed fact |
|---|---|---|
| reviewed tool spec | `PASS` | spec id `vera20k-oracle-tools-2026-07-10.1`, SHA-256 `0CB86F8F...E5D162C` |
| local root | `PASS` | `<local>/AppData/Local/VERA20k/oracle`, inspected without mutation |
| local tool lock | `LOCAL_TOOL_LOCK_INVALID` | lock `spec_sha256` differs from reviewed spec |
| native capture build | `NATIVE_CAPTURE_INVALID` | enrolled `build.ps1` expected `89531586...B146C`, observed `1A8A4CFD...FC18` |
| native capture enrollment | `NATIVE_CAPTURE_UNENROLLED` | valid lock and valid build are both required |
| debugger port | `PASS` | `127.0.0.1:8099` was free; no listener was started |
| host environment | `PASS` | Windows/QPC/monitor/DPI/foreground/overlay facts recorded read-only |
| Ghidra GUI runtime | `UNCHECKED` | intentionally not started |
| runtime smokes | `UNCHECKED` | no debugger, capture, PresentMon, Procmon, or gamemd smoke executed |

Summary: three required failures, four passes, and two unchecked optional runtime
checks.

### 5.4 Workspace and startup lifecycle

`workspace-status --json` returned `READY` with all four checks passing and linked
the private oracle to the public VERA20k root. This is only path/topology readiness;
it does not override `doctor` or pipeline blockers.

`startup-lifecycle-recipe --include-back-reselection` returned the bounded shell
sequence main menu → single player → skirmish → Back → skirmish → first Start,
requires the same PID, and uses `--args=-WIN`. It also returned:

- `fixture_configuration_owned=false`;
- `sealed_runtime_blocked_without_configurator=true`;
- `second_start_exposed=false`.

Therefore the shell lifecycle helper cannot presently seal the required movement
fixture.

## 6. Existing Protocol, Transport, Hook, and Fixture Audit

### 6.1 StateSnapshot is an offline codec, not captured evidence

`tools/oracle_protocol/src/snapshot_v2.rs` defines a fixed 3,416-byte kind-5
`StateSnapshot` and tests exact encode/decode behavior. The protocol README states
that the instrument does not call the state-snapshot-v2 encoder; emission is
explicitly unclaimed.

The current `MtnkState` block contains health, position, abbreviated body-facing
state, mission-labelled fields, three NavCom-related pointers, destination,
head-to, and flags. It does not contain the full Checkpoint-E capture envelope.
Moreover, the older design prose still says 3,424 bytes and uses stale mission
labels. The current codec and its 3,416-byte golden control its own wire shape, but
neither source establishes correct native semantics for the omitted or mislabeled
movement fields.

### 6.2 Exact transport correction

There are two different transports and they must not be conflated:

- legacy `transport.rs` has 128-byte slots with 120 bytes of record capacity;
- `startup_transport.rs` has 4,096-byte slots with 4,080 bytes of record capacity.

The 3,416-byte StateSnapshot cannot fit the legacy tick transport, but it **does**
fit one existing large startup-transport slot. The actual large-slot record capacity
leaves `4,080 - 3,416 = 664` bytes. Separately, `snapshot_v2.rs` freezes a
`4,096 - 8 - 3,416 = 672` calculation for its 4,088-byte codec test payload; that
test must not be mistaken for the startup transport's 16-byte slot prefix. The
blocker is not an unavoidable need for fragmentation. The blocker is that no
reviewed live movement producer routes kind 5 through the large transport, and the
schema still lacks the Checkpoint-E fields and ordered event model.

The implementation decision may reuse and generalize the large transport or define
bounded compact records. It must not claim the legacy 120-byte payload already
carries StateSnapshot.

### 6.3 Existing hook coverage

The current startup-observed manifest contains:

| Hook | Address | Verified role in manifest | E coverage |
|---|---:|---|---|
| `bootstrap_once` | `0x007CD84D` | process-entry bootstrap | identity/startup only |
| `seed_control` | `0x0052FDF9` | selected seed authority load | startup RNG authority only |
| `session_ack` | `0x006AD8C7` | offline Start/Back session gate | session correlation only |
| `tick_complete` | `0x0055DEA9` | completed native tick after four terminal guards | completed-tick boundary only |

It contains no command-dispatch receipt and no object-pass, Unit, Foot, locomotor,
cell-cross, arrival, lifecycle, or later-object-effect hook.

### 6.4 Existing hook and transport smokes

Two sealed July 11 hook/transport smokes, taken together, prove bounded
infrastructure facts. The earlier two-hook ABI smoke proves exported-atomic and
`ReadProcessMemory` observations plus continuation; it does not use shared-memory
transport. The later transport smoke proves hook/trampoline/frame-label behavior
and shared-memory frame-record delivery. Both ended through forced teardown and
explicitly do not prove movement state, RNG state, pixels, or parity.

### 6.5 Existing MTNK recipe

`mtnk-empty-cell-move.recipe.v1.json` requests Battle/America at 640×480,
GameSpeed 3, one opponent, 10,000 credits, ten starting units, and a unique stock
MTNK commanded to an empty cell. Its candidate maps are `CrctBrd.yro`,
`DeepFrze.yro`, and `HighExpR.yro` in sorted stock-first order.

The finalized `mtnk-empty-cell-move.v1.json` scenario is absent. The recipe does not
seal an exact map hash, starting cell, destination cell, initial facing, object ID,
LogicVector index, terrain, owner/veterancy/crate state, or hidden RNG state. It is a
configuration request, not a reproducible movement fixture and not parity evidence.

The local oracle checkpoint directory contained zero files at preflight time.

### 6.6 Retail fixture seed inventory

The retail install is sufficient to seed some fixtures, but it does not contain a
ready saved movement checkpoint or replay. A bounded read-only scan found no
`.sav`, `.rpl`, `.rep`, `.yrp`, or `.rp2` artifact usable as common initial state.
The install contains 16 MIX archives and 54 loose `.mmx/.yro/.map` maps.

The runtime companion identities observed across the original preflight and final
review are:

| File | SHA-256 | Role / warning |
|---|---|---|
| `ddraw.dll` | `2F1399EF5E6CDBA02495FBB66C731924C9A7B0B40B43B8B496DFB18993328039` | presentation/capture environment identity |
| `DDrawCompat.ini` | `AB96FDEFE27D7E6E185AD7AA50AB94B903C8C742CAE38425BA63D6FA6E76300E` | presentation configuration identity |
| `RA2MD.INI` | original-preflight SHA-256 `466DF459C8464700EE4B9B5CAA8739D9D3BCA35B26C5A6AEB1E0B1C1E76F1BB6`; final-review SHA-256 `792ABE03D8A5B67E02A758E8ACB1B643583CB4C79DA2C566553478A74855549F` (file mtime `2026-07-22 21:47:00 +02:00`, observed `2026-07-22 22:13:05 +02:00`) | mutable user shell/skirmish state; never a sealed fixture by itself |

The strongest stock seeds are:

| Required family | Retail seed | Verified static facts | Runtime limitation |
|---|---|---|---|
| AMCV baseline | `DeepFrze.yro`, SHA-256 `1904D7337ED2EAE8AC321E2B31FBBE6B7E3117DD8C99C3C911DBED09931F8B8A` | four starts; no preplaced Unit/Infantry rows; with UnitCount 0 and Bases enabled, stock data makes this a candidate where the local America player's only starting unit is expected to be AMCV | the opponent has its own base unit; local spawn, exact start, facing, destination, seed, object identity, and live order remain runtime facts to seal |
| MTNK | same exact stock map is a clean candidate | `[MTNK]` is Allied, TechLevel 2, Drive, and has no explicit `AllowedToStartInMultiplayer=no` | native budgeted candidate selection is random; UnitCount 10 does not guarantee one unique MTNK |
| Walk idle/host | `Dustbowl.map`, SHA-256 `46B07F8968BE4C267CBDEC5B99CF36E9BDE98F4AC0D23B7D634ABF86E9165A79` | 33 neutral Walk infantry; 13 ordinary civilians and 20 COW, with COW `Crushable=no` | static preplacement does not prove controlled movement or exact live order |
| Walk Move seed | `IrvineCa.yro`, SHA-256 `086DEF8929B6E65E9E13704E908DE189BB189C1BD9BB27D3B99E2F3AD2867767` | 265 neutral Walk infantry; stored Mission=Move CIV2 at `(99,128)` and CIV3 at `(102,132)` | useful host/arrival seed, not a certified commanded-movement fixture |
| Ship idle | `CrctBrd.yro`, SHA-256 `039EB0A5F40AAF33AB3B308123B84ED277456A2846E9F527D8DE32EB5728A58F` | neutral CRUISE at `(123,125)` and `(103,99)`; neutral TUG at `(115,98)`, `(132,110)`, `(103,122)`; all Sleep | proves a stock idle-Ship seed, not a controllable move fixture |
| Hover | none among 54 loose maps | zero SAPC/LCRF/YHVR/ROBO preplacements; all four say `AllowedToStartInMultiplayer=no` | requires build, save, or purpose-built placement |
| miner/refinery | none ready among 54 loose maps | zero relevant miner/refinery preplacements; normal stock creation is GAREFN `FreeUnit=CMIN` or NAREFN `FreeUnit=HARV` | requires construction and sealed live order |
| stock low-bridge route | `Carville.mmx`, SHA-256 `3268FD2DD00E0D572497F3CE1540B204929232D92E1A21EEC8179BE6D6A74796` | verified low-bridge route seed | requires final-collapse/route logging; it is not an explicit Tube path |
| explicit multi-step Tube | no stock seed | zero `[Tubes]` in 54 loose maps; final C reports zero across 385 retail maps | requires a custom/mod fixture and must be labeled conditional, not stock-normal |

`rulesmd.ini:390` supplies `BaseUnit=AMCV,SMCV,PCV`.
`rulesmd.ini:6603..6648` supplies the MTNK stock metadata.
`rulesmd.ini:11722..11744` and `:12515..12538` supply the Allied/Soviet refinery
free-unit routes.

The existing recipe's `require_unique=true` is therefore a requirement, not a
proved condition. The checkpoint builder must observe and seal the chosen MTNK or
fail; it must not retry invisibly until a desired random roster appears without
recording the seed, attempts, and resulting authority state.

## 7. Universal Machine-Readable Capture Contract

Each run must have one immutable fixture manifest, ordered event records, bounded
pre/post snapshots, and a result manifest that hashes every member. A single final
state blob is insufficient because the load-bearing evidence is intermediate order,
same-Process retry, same-tick lifecycle mutation, and what a later object observes.

### 7.1 Bundle identity

Every bundle must record:

- schema and fixture version;
- retail executable SHA-256 and size;
- exact map filename and content hash;
- `rulesmd.ini` and base-fallback hashes used by the run;
- private oracle HEAD and dirty-scope disposition;
- tool-spec, hook-manifest, producer, collector, and scenario hashes;
- capture lane (`instrumented-state` or `clean-presentation`);
- run ID, monotonic record sequence, QPC frequency, wall timestamp, and process ID;
- hook static address, runtime address, module identity, and callback schema;
- explicit completion/overflow/forced-teardown status.

### 7.2 Per-object and host state

At each relevant invocation and return, record:

- capture-local stable object ID, native pointer, type ID, owner, and locomotor kind;
- LogicVector index and count before and after; whether the object actually ran;
- `g_CurrentFrameCounter`, live GameSpeed byte, and QPC timestamp;
- `ObjectClass::AI` entry/return and owner `+0x90` before and after it, including
  whether Object AI cleared `+0x90` before Mission timer evaluation;
- mission current, suspended, queued, raw commenced byte, substate, MissionTimer
  start/raw auxiliary, dispatch timer start/raw auxiliary/rate;
- timer not-due/due and due-health gate outcomes, including `health <= 0`;
- raw `+0xCC` storage only if captured; do not assign it a semantic role;
- owner active byte `+0x90`, health `+0x6C`, queued mission `+0xB4`, and arrival guard
  `+0x6B3`;
- Unit wrapper receipts in native short-circuit order: read/save `+0x6E0`, clear
  `+0x6D2`, check saved `+0x6E0`, conditionally read/check `+0x6E1`, then
  conditionally read/check `+0x6E2`; on every taken short circuit record the
  `+0x1E8(5,0)` call, return value `1`, and zero Mission_Move RNG consumption;
- after a Techno guard-B or guard-E return, Foot's immediate post-Techno `+0x90`
  read/fail and the absence of Foot pre-work;
- Foot gates `+0x674`, `+0x3CD`, `+0x8D`, `+0x2A8`, type `+0x692`, and `+0x81` in
  the exact tested order;
- coordinates `+0x9C/+0xA0/+0xA4`, semantic body/turret facing and raw facing state;
- NavCom `+0x5A4`, auxiliary target `+0x5A0`, suspended target where relevant, and
  the complete path sequence, not merely its head.

`+0x90 != 0` is an active-byte observation. It must not be renamed “alive object”
or “not physically deleted” without the corresponding lifecycle evidence.

### 7.3 Drive complete-object state

For Drive entry, every DriveTrack attempt, retry, and return, record:

- destination `+0x34..+0x3C`;
- head-to `+0x40..+0x48`;
- residual `+0x4C`;
- target speed fraction qword `+0x50..+0x57`;
- RawTrack selector `+0x58`;
- RawTrack cursor `+0x5C`;
- short byte `+0x60`;
- owner current speed fraction `+0x578/+0x57C`;
- fresh integer returned by exact `GetCurrentSpeed` conversion;
- point index before consumption, spend, coordinate/facing writes, cell transition,
  track-complete status, and whether the same Process retries.

### 7.4 Population, occupancy, and lifecycle

Where relevant, record:

- tube direction/index `+0x684/+0x685` and locomotor piggyback state;
- active byte, InLimbo state, live-Logic membership, live-vector cursor/count;
- CellClass linked-list head, saved-next traversal order, occupancy/Mark calls, and
  building/gate/factory contact state;
- pending-delete queue membership and physical-drain receipt;
- ordered Reveal/Conceal/UnInit/Limbo/list-removal and cache/zone/bridge receipts;
- ordered sound, Scatter, crush, wall/overlay, radar/tracker/dirty, flash, and
  later-object-run receipts.

### 7.5 RNG

Record the complete logical state of Scenario RNG at `Scenario+0x218` at each
defined snapshot boundary, plus:

- logical API-call count;
- raw `next_u32()` advance count;
- every candidate consumed by a rejection loop;
- every accepted ranged value and the branch that consumed it.

One ring index or `(after-before) mod 250` is insufficient because it aliases 250
or more raw draws and one ranged API call may reject more than one word.

### 7.6 Command authority

Native input enqueue timestamps do not prove command consumption. Each commanded
fixture needs a native dispatch receipt tied to the same object, destination,
correlation ID, and monotonic record sequence. The existing snapshot design names a
`CommandConsumed` record, but the current runtime does not emit it and the exact
movement command hook must be re-frozen before enrollment.

## 8. Required Fixture and Discriminator Matrix

### 8.1 Frame and pacing

| Fixture | Required variants | Required observations | Closes | Does not close |
|---|---|---|---|---|
| normal frame | local GameSpeed `0..6` | object-pass frame N, normal commit N→N+1, QPC/present intervals | measured cadence/jitter and frame placement | locomotor semantics |
| late terminal flag 1 | first native flag test in `0x0055DE4F..0x0055DE71` | pass ran at N; identify flag/test; no increment, wait, or remaining normal tail | first flag's shared terminal branch | flag owner semantics beyond this slice |
| late terminal flag 2 | second native flag test in the same range | pass ran at N; identify flag/test; no increment, wait, or remaining normal tail | second flag's shared terminal branch | flag owner semantics beyond this slice |
| late terminal flag 3 | third native flag test in the same range | pass ran at N; identify flag/test; no increment, wait, or remaining normal tail | third flag's shared terminal branch | flag owner semantics beyond this slice |
| late terminal flag 4 | fourth native flag test in the same range | pass ran at N; identify flag/test; no increment, wait, or remaining normal tail | fourth flag's shared terminal branch | flag owner semantics beyond this slice |
| negative controls | optional Scenario `+0x62C`, offline modal state | whether object pass or wait path changes | scoped cadence branches | all game modes |

One generic terminal sample closes only the common branch. If all four flags cannot
be reached without unauthorized state mutation, the exhaustive four-flag runtime
claim must remain withheld rather than being inferred from one exit.

### 8.2 Host branch matrix

Capture exact entry/return order for:

1. Mission timer not due;
2. timer due with NavCom;
3. timer due with `health <= 0`;
4. `ObjectClass::AI` clearing `+0x90` before the timer path;
5. stopped, NavCom null, no queued mission;
6. stopped, NavCom null, queued mission;
7. each Unit saved-byte short circuit with the exact sequence read/save `+0x6E0` →
   clear `+0x6D2` → check saved `+0x6E0` → conditional read/check `+0x6E1` →
   conditional read/check `+0x6E2`;
8. for every Unit short circuit, `+0x1E8(5,0)`, return `1`, and no Move RNG;
9. Techno guard B exit followed by Foot's immediate post-Techno `+0x90` read/fail,
   with no Foot pre-work;
10. Techno guard E exit followed by the same Foot guard/fail boundary;
11. each of the five immediate Foot Process gates;
12. locomotor Process clearing `+0x90`;
13. a Mission_Move ranged draw that rejects candidate `3` before accepting `0..2`.

This family can turn the cloned Checkpoint-A host trace into retail-backed contract
evidence. It does not by itself prove the later Unit tail or production authority
cutover.

### 8.3 AMCV open-ground turn

Preferred discriminator: stock AMCV at cell `(40,40)`, semantic facing `0`, target
cell `(45,40)`, destination/path present but no pre-existing track. Capture the
first Process selecting TurnTrack 2 / RawTrack 4 at cursor 0, the zero-budget first
turn state, every point application, cell transitions, and arrival.

Required negative controls include an already-initialized track and a non-turning
straight target so track ownership is not inferred from one branch.

### 8.4 MTNK point-budget and cell-boundary run

Capture a flat, clear straight movement that distinguishes:

- entry budget `22`;
- one cell-cross point leaving `8`;
- same-Process continuation leaving residual `1`;
- companion budgets `7` and `8` for the strict `> 7` decision;
- residual `3` and `4` for the interpolation split.

This closes the scoped point-budget, current-point-before-increment,
cell-cross-continuation, and interpolation comparisons. It does not certify A* or
static-wall classification.

### 8.5 Exact speed-stage matrix

Separate four evidence classes; do not blend synthetic boundaries into stock runs:

1. stock AMCV baseline (`Trainable=no`), with stock fractions only;
2. stock MTNK rookie/veteran/elite and Speed-crate states, plus distinguishing
   current fractions and retry re-query;
3. explicitly modded INI parser boundaries (`-1`, other negatives, `99`, `100`);
4. corrupt/synthetic setter zero/one/NaN/infinities, CTF signed-negative cases,
   valid wide signed-64 values, and invalid conversions.

The selected read-only observer cannot create class 4 inputs. Those cases require a
separately authorized native state/call producer or remain deferred. At conversion
call sites `0x004DB1DB` and `0x004DB213`, distinguish the pre-call x87 operand from
the post-return EAX result; do not describe both as a value “at” the call. This
closes captured x87/store-boundary outputs only, not the upstream
terrain/slope/health fraction producer or the complete CTF subsystem.

### 8.6 Retry, sentinel, and arrival

Required variants:

- track completion → ProcessMovement → `Process_Drive_Track(1)` in the same
  locomotor Process;
- two speed-state queries with the retry's fresh integer masked by its flag;
- paid `(0,0)` sentinel;
- empty next-slot arrival;
- queued next-slot continuation.

Capture Unit arrival wrapper `+0x480`, `Stop_Moving`, `OnArrival`, exact coordinate
commit, NavCom/path mutation, and what the next live object observes. Do not
generalize this fixture to untraced class-specific arrival wrappers.

### 8.7 RawTrack initializer matrix

Required variants:

- fresh raw selector 3 and fallback raw selectors 1–2;
- raw 3 cursor 37 → raw 4 anchor 11 survivor;
- separate `+0x90`, `+0x81`, and `+0x8D` exits retaining cursor 10;
- raw 3 point `+0x0C=22` before, at, and after the relevant threshold;
- TurnTrack flags 8 versus 0;
- Force with a nonzero anchor, proving residual/short preservation and target
  fraction behavior.

The missing fresh producer of short byte 1 remains a static/runtime research gap
unless the capture observes and attributes it.

### 8.8 Conditional save/load

This fixture becomes mandatory if the cutover changes persistence or claims
support for in-flight Drive, Tube, forced, or piggyback state. Save immediately
before a chain threshold or active special state; compare the first two post-load
object turns, next point, and effect order.

If persistence is not changed, explicitly withhold in-flight save parity rather
than calling it verified. This conditional scope reflects final Checkpoints C and
D and supersedes unconditional language in older planning prose.

### 8.9 Mixed exact-once and idle population

Interleave idle and active Drive, Infantry/Walk, conditional Unit/Walk, Hover, Ship,
Teleport, Tube, forced, and miner objects in known LogicVector order. Include parked
Walk, Hover, Ship, and Teleport objects. For the ordinary exact-once baseline,
disable the Unit/Infantry class-special `+0x1D8/+0x1D4/+0x27C` paths. In separate
special-path fixtures, record the legitimate earlier locomotor `+0x40` call, the
re-evaluated predicates, the later Unit Tube test, and whether normal Foot makes a
second call. Do not label a legitimate class-special call plus Foot call as an
oracle double-run defect.

This closes exercised population/precedence only; it does not certify the complete
numeric algorithm of every locomotor.

### 8.10 Teleport and CMIN restoration

Required variants:

- idle CLEG;
- accepted CMIN empty cell;
- accepted Infantry-only cell;
- Unit-occupied rejection;
- `Is_Piggybacking` false;
- `Is_Ok_To_End` false;
- far Drive staging;
- late restore followed by later reissue.

Capture contact/HELLO/CAN_DOCK/`0x12`, `Set_Destination`, immediate restoration
gates at `0x00742534` and `0x00742554`, and late restoration at `0x004DAE5F`.

### 8.11 Forced callers and conditional defaults

Exercise and label separately:

- factory selector 66 with direct Drive;
- factory selector 66 with Teleport-primary outer Drive;
- bunker selectors 67–70;
- reciprocal-link selector 71;
- healthy zero-link miner negative control;
- trigger action 128 relocation with selector `-1`;
- IsLocomotor replacement with selector `-1`;
- custom/mod instantiation of DeathDummy and YDUM, which native metadata defaults
  to Teleport;
- zero-speed idle Teleport;
- unmultiplied GI/Conscript controls.

Do not claim stock reachability for selectors 64/65 or shipped instance producers
for DeathDummy/YDUM without new evidence.

### 8.12 Miner and convoy order

Run two miners A/B contending for one refinery in both live orders and with timer
due/not-due variants. Capture mission mutation and the same object's subsequent
locomotor call. Separately capture explicitly linked Drive convoy fraction
propagation and an unlinked same-command-group negative control.

### 8.13 Live-vector mutation

Use the final D fixtures:

1. victim after the live cursor;
2. victim before or at the cursor;
3. tail Reveal/insertion.

Record vector count/index/pointer changes and exact run/skip order. These samples do
not exhaust Detach listeners or BREAK receiver subclasses.

### 8.14 Crush, Scatter, and accepted-owner death

Required variants include two victims in CellClass list order, a crusher whose
state changes mid-list, direct `+0xF8` Infantry handling, an accepted-cell callback
that kills the mover, and mixed Scatter eligibility with exact RNG draws. Record
saved-next traversal, sound placement, lifecycle writes, and later-object effects.

### 8.15 Later-object movement effects

Capture:

- Infantry target A moves before following Unit B;
- a finished mover before an observer;
- empty and queued arrival positioned both before and after the observer.

These fixtures prove dynamic re-aim, completion visibility, and ordinary-arrival
ordering for the exercised paths only.

### 8.16 Gate, factory, and wall effects

Capture both gate order inversions, factory contact removal before a second
same-budget cell entry, and a BFRT fully entering and leaving a wall cell. Preserve
the observed sound → `DestroyOverlay(-1)` → rocking order, request/progression,
contact, and overlay/list visibility.

Alternate wall predicates and the full DestroyOverlay subsystem remain outside the
proved slice.

### 8.17 Tube lifecycle, speed, and display effects

First capture the direction-8 producer and prove consumption on the next object
turn. At Tube entry, Drive selector remains `-1` and Tube cursor `+0x685` begins at
zero. Record Unit budget `ftol(TypeSpeed * 1.5)` versus Infantry raw-TypeSpeed
budget, residual carry, at most one cursor increment per object turn, and no
forced/ordinary resumption after Tube completion in that turn.

Capture these branch-specific call orders rather than one universal slash-list:

- Unit success: optional `+0x174` → `+0x18C` → `+0x544(1.0)` → `+0x4A0`;
- Infantry ordinary success: `+0x544(1.0)` → `+0x18C` → `+0x4A0`;
- Infantry NavCom-equal success: `+0x174` → `+0x18C` → `+0x4A0`, preserving a
  prior fraction such as `0.375`;
- blocked: occupant `+0x174` callbacks → `+0x544(+0.0)` → `+0x4A0`, with no
  completion `+0x18C`;
- garrison-conceal: the proved Infantry path, still reaching `+0x4A0`.

For each applicable branch also record cross bit 2, Tube sign, visibility/out-code,
discovery `+0x41B`, the unconditional radar rectangle query, type-5 packed-coordinate
call, `+0x423`, `+0x208/+0x20C`, tracker removal/addition, dirty call, tracker state,
and flash due/not-due.

This does not exhaust the full `+0x324` visibility input space or prove all
downstream tracker/pixel equivalence.

### 8.18 Cache, bridge, ordered outputs, and deletion

Capture:

- early-unmarked victim cache split;
- ordered Conceal effects for selection, display, animation, Voc, dirty/drawn, and
  redraw outputs;
- clear plus each blocked deferred-vehicle bridge result, recording
  `pending_bridge_update`, `OnBridge`, bridge occupancy, selected CellClass list,
  and the next observer's layer read;
- full crush at frame N, later-object absence at N, state at N+1, and physical
  drain receipt.

These fixtures close the exact sampled cache/output/bridge/drain seams, not the
broader route/zone/cache graph, non-ground death behavior, or unrelated save/load
reconstruction.

## 9. Probe Spine Inherited from Frozen Research

No fresh Ghidra mutation or runtime attach occurred. These are reviewed static
anchors inherited from Checkpoints A–D and must be independently revalidated in the
hook-enrollment review:

| Area | Static anchor(s) | Intended capture role |
|---|---|---|
| Main Tick | `0x0055D360` | top-level runtime owner |
| live pass call | `0x0055DC9E → 0x0055AFB0` | one object-pass boundary |
| live iteration | `0x0055B5FB..0x0055B619` | LogicVector order/mutation |
| frame increment | `0x0055DE73..0x0055DE81` | normal N→N+1 commit |
| wait | `0x0055E160` | pacing boundary; clean-lane measurement only |
| pending-delete callsite | `0x0055DE9F` | deferred physical drain |
| Unit host | `0x007360C0` | Unit wrapper and tail |
| Foot host | `0x004DA530` | per-object locomotor owner |
| Techno host | `0x006F9E50` | common pre/post guards |
| Mission Dispatch | `0x005B3060` | mission/timer ordering |
| Unit Move wrapper | `0x00740A90` | saved-byte short circuits |
| Foot Mission Move | `0x004D4200` | NavCom/queue/arrival/RNG branches |
| Drive Process | `0x004B0500` | locomotor entry/return |
| DriveTrack | `0x004B0F20..0x004B2605` | point budget, track, retry, effects |
| GetCurrentSpeed | `0x004DB1A0..0x004DB245` | exact staged conversion |
| Drive Force | `0x004B0C40..0x004B0D9F` | forced metadata initialization |
| Walk Process | `0x0075AC80` | class Process receipt |
| Hover Process | `0x00514310` | class Process and vertical state |
| Ship Process | `0x0069FC10` | class Process receipt |
| Teleport Process | `0x007192F0` | class Process/piggyback state |
| Unit Tube | `0x007359F0` | tube entry/body |
| Infantry Tube | `0x0051B350` | tube entry/body |
| Unit per-cell | `0x00739EC0` | cell acceptance/effects |
| crush | `0x007416A0` | victim order and lifecycle |
| Scatter | `0x00481670` | eligibility/RNG/order |
| Conceal | `0x005F4D30` | lifecycle/effect ordering |
| UnInit | `0x005F65F0` | logical removal owner |
| Logic removal | `0x0055BAE0` | live-vector mutation |
| physical drain | `0x00725C70` | delete timing |
| Tube suffix | `0x0070D990` | visibility/tracker/display effects |

Installing all of these as hot hooks at once is not automatically safe. The
tooling design must minimize hook count, derive bounded records, and use calibration
runs to prove each callback's receiver and preservation contract before fixture
runs.

## 10. Reproducibility and Acceptance Rules

### 10.1 Run lanes

Each sealed fixture should have, at minimum:

- two instrumented state/order repeats with byte-identical normalized event
  sequences where deterministic;
- three clean presentation/cadence repeats with no debugger and no injected state
  observer;
- explicit failure if identity, command receipt, hook count, overflow status, or
  fixture precondition differs;
- a clean negative-control run where the distinguishing branch is not taken.

Repeat counts are a minimum stability check, not a statistical proof of parity.

### 10.2 Comparison authority

The eventual Rust comparator must consume immutable retail artifacts. It may not
derive expected values from current Rust, from hand calculation, or from prose.
Rust-versus-prior-Rust hashes remain regression ratchets only.

Pointer values may need run-local normalization, but pointer identity/order within a
run must not be discarded. Any normalization rule must be versioned and unable to
hide object substitution or pointer reuse.

### 10.3 Failure states

At minimum, a fixture is invalid if any of these occur:

- executable, map, rules, tool, manifest, or scenario hash mismatch;
- missing/duplicate/out-of-order startup or command authority records;
- target object ambiguity, pointer reuse, or unexpected type/owner/locomotor;
- missing hook hit, excess hook hit, sequence gap, ring overflow, partial record,
  CRC failure, or forced teardown;
- debugger or instrumented data used as clean cadence evidence;
- absent complete Scenario RNG state where a conditional draw is in scope;
- a branch precondition was inferred rather than recorded;
- final state matches but required intermediate order was not captured.

## 11. Current Rust / Production Status

This report made no Rust comparison beyond the frozen implementation contract and
the final D handoff. The public `src/sim/world/techno_ai.rs` working-tree change was
companion-owned and was not treated as a stable baseline.

Production activation remains NO-GO because:

1. no executable retail movement fixtures exist;
2. the final C population includes ground Teleport, while older design language
   still needs reconciliation;
3. final D leaves bounded lifecycle/effect research blockers and requires atomic
   retirement of old owners;
4. no comparator currently consumes retail movement artifacts;
5. Checkpoint E cannot certify a behavior-bearing change until the oracle tooling,
   scenarios, captures, and observer proof exist.

The existing cloned/test-only host harness may remain regression evidence. It is not
production authority and cannot substitute for retail captures.

## 12. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| retail executable identity | verified | direct SHA-256/size read | repeat identity in every bundle |
| public/private repository snapshots | verified | `git rev-parse`, `git status` | freeze again before implementation |
| oracle facade topology | verified | `oracle.py status --json` | none for read-only topology |
| oracle command safety classes | verified | `oracle.py capabilities --json` | separate approvals for mutation/runtime commands |
| workspace link | verified | `workspace-status --json` | does not close runtime readiness |
| local tool enrollment | verified | `oracle.py doctor` | repair reviewed lock and native capture enrollment |
| shell lifecycle recipe | verified | `startup-lifecycle-recipe` | implement owned fixture configurator |
| MTNK recipe existence | verified | recipe hash/content | create sealed scenario with exact preconditions |
| finalized MTNK scenario | verified | absence check | implement `create-checkpoint` and seal artifact |
| checkpoint directory | verified | read-only directory listing | no captures exist |
| original-YR runner | verified | status/CLI source | implement supported `run-original` path |
| cross-engine comparator | verified | status/registry source | implement comparator and VERA adapter |
| input authority integration | touched-not-exhausted | status plus transient dirty WIP | wait for owner; review settled implementation separately |
| startup hook manifest | verified | manifest content/hash | add separately reviewed movement hooks |
| movement hook manifest | verified | absence across manifests | design, verify, enroll bounded hooks |
| legacy 128-byte transport | verified | `transport.rs` constants | cannot carry 3,416-byte record |
| large 4,096-byte transport | verified | `startup_transport.rs` constants | integrate/review for movement producer |
| StateSnapshot wire length | verified | codec/README/golden | 3,416 bytes; correct stale 3,424 prose when design is revised |
| StateSnapshot live producer | verified | protocol README/source search | absent; implement and certify |
| current MTNK state fields | verified | `snapshot_v2.rs:183..207` | replace stale semantics and add E fields |
| full three-RNG codec | verified | `snapshot_v2.rs` | connect to atomic live producer and receipts |
| command-consumption receipt | touched-not-exhausted | snapshot design only | re-freeze native hook and implement producer |
| existing transport smokes | verified | transport report and run inventory | infrastructure only; not movement evidence |
| debugger collector | verified | collector source | diagnostic calibration only |
| native DXGI collector | touched-not-exhausted | collector source and failed enrollment | re-enroll before clean lane |
| atomic external memory sampling | verified | snapshot design rejection | do not use as final state oracle |
| callback-local machine-state preservation | verified | instrument README:100..105; `callback_shim.rs:1..7` | FXSAVE/LastError are preserved inside current shims |
| SyringeEx post-callback TEB write | verified | same README/source boundary | loader-side `FS:[0x14]` preservation or positive native non-consumption/equivalence proof |
| observer non-perturbation | deferred | snapshot design plus known `FS:[0x14]` blocker | close installed-shim blocker, then design and execute sealed proof |
| universal capture envelope | verified | A–D reports and Checkpoint-E plan | encode in versioned schemas |
| frame/GameSpeed capture family | verified | scheduling report and plan | clean GameSpeed captures and four individually distinguished terminal flags absent |
| host branch family | verified | host contract | runtime captures absent |
| AMCV turn family | verified | AMCV retrace/contract | sealed fixture and runtime captures absent |
| MTNK boundary family | verified | speed-budget report/contract | sealed fixture and runtime captures absent |
| stock exact speed-stage family | verified | exact-speed report and stock INI | runtime stock discriminators absent |
| synthetic/corrupt speed-stage family | deferred | exact-speed report boundaries | requires separately authorized state/call producer or withheld claim |
| retry/arrival family | verified | scheduling/RawTrack/D reports | runtime captures absent |
| RawTrack initializer family | verified | RawTrack report | runtime captures absent |
| save/load family | verified | final C/D conditional wording | required only if persistence/support changes |
| mixed locomotor population | verified | final C report | runtime exact-once captures absent |
| Teleport/CMIN family | verified | final C report | runtime captures absent |
| forced/default family | verified | final C report | runtime captures absent |
| miner/convoy family | verified | final C report | runtime captures absent |
| live-vector mutation family | verified | final D fixtures 1–3 | runtime captures absent |
| crush/Scatter family | verified | final D fixtures 4–8 | runtime captures absent |
| later-object visibility family | verified | final D fixtures 9–11 | runtime captures absent |
| gate/factory/wall family | verified | final D fixtures 12–14 | runtime captures absent |
| Tube lifecycle/display family | verified | final D fixtures 15–17 | runtime captures absent |
| cache/bridge/delete family | verified | final D fixtures 18–21 | runtime captures absent |
| Detach/listener exhaustion | deferred | final D blocker | focused research before claiming full family parity |
| BREAK receiver/subclass exhaustion | deferred | final D blocker | focused research before claiming full family parity |
| full cache/zone/deferred-bridge graph | deferred | final D blocker | focused research before broad parity claim |
| alternate DestroyOverlay predicates/effects | deferred | final D blocker | focused research before broad wall parity claim |
| all class-specific arrival wrappers | deferred | final D blocker | trace each production-relevant wrapper |
| full `+0x324`/tracker/pixel semantics | deferred | final D blocker | exhaustive visibility/presentation slice |
| non-ground lifecycle regressions | deferred | final D blocker | separate shared-lifecycle regression scope |
| actual retail movement captures | deferred | runtime/tooling/approval gate | implement reviewed oracle, then execute E |
| Rust artifact adapter/comparison | deferred | no retail bundle exists | implement only after schema/bundle freeze |
| production cutover certification | deferred | depends on E plus static caller review | no production activation before both pass |

## 13. Open Questions — Final State of the Investigation Log

- `[RESOLVED] E-OQ-01 — What executable is the oracle target? → The 5,286,504-byte retail gamemd.exe with SHA-256 1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C.` (evidence: `direct Get-Item/Get-FileHash; private hook manifests`)
- `[RESOLVED] E-OQ-02 — Is the private oracle topology currently valid? → Yes, the registry reports VALID topology, but parity authority is NONE.` (evidence: `oracle.py status --json; registry SHA-256 04B7AB...C61EDF`)
- `[RESOLVED] E-OQ-03 — Can the supported facade create the MTNK checkpoint today? → No; create-checkpoint is a registered STUB and the finalized scenario is absent.` (evidence: `oracle.py status --json; oracle-system.v1.json; scenario-directory read`)
- `[RESOLVED] E-OQ-04 — Can the supported facade run the original-YR oracle today? → No; run-original is a registered STUB and the original-YR execution pipeline is BLOCKED.` (evidence: `oracle.py status --json; oracle-system.v1.json`)
- `[RESOLVED] E-OQ-05 — Does any existing artifact capture live retail movement state? → No; checkpoints are empty and existing sealed smokes cover transport infrastructure only.` (evidence: `local checkpoint listing; LOGGER_DLL_RUNTIME_OBSERVATION_TRANSPORT_REPORT.md`)
- `[RESOLVED] E-OQ-06 — Does the current instrument emit StateSnapshot v2? → No; emission is explicitly unclaimed.` (evidence: `tools/oracle_protocol/README.md:54..61; source search of oracle_instrument`)
- `[RESOLVED] E-OQ-07 — Is the StateSnapshot record 3,424 or 3,416 bytes? → The current codec/golden authority is exactly 3,416 bytes; 3,424 in the older design is stale.` (evidence: `snapshot_v2.rs:11,1013..1014; tools/oracle_protocol/README.md:27..31`)
- `[RESOLVED] E-OQ-08 — Can the current 3,416-byte snapshot fit any existing transport slot? → It cannot fit legacy 120-byte record capacity, but it fits the large transport's 4,080-byte capacity.` (evidence: `transport.rs:19..21; startup_transport.rs:24..27; snapshot_v2.rs:1013..1014`)
- `[RESOLVED] E-OQ-09 — Is transport capacity the only StateSnapshot blocker? → No; no live producer routes it, its MTNK semantics are stale/partial, and it lacks required movement/event fields.` (evidence: `protocol README:54..61; snapshot_v2.rs:183..207; A–D capture requirements`)
- `[RESOLVED] E-OQ-10 — Do current manifests hook movement? → No; they hook bootstrap, seed, session acknowledgement, and completed tick only.` (evidence: `startup-observed hook manifest`)
- `[RESOLVED] E-OQ-11 — Can SendInput timing prove the move was consumed? → No; a correlated native command-dispatch receipt is required.` (evidence: `oracle-rng-mtnk-state-snapshot-design.md:45..48,381..383`)
- `[RESOLVED] E-OQ-12 — Can external ReadProcessMemory be the final state oracle? → No; it cannot prove one atomic completed-tick view across the required state graph.` (evidence: `oracle-rng-mtnk-state-snapshot-design.md:1360..1366`)
- `[RESOLVED] E-OQ-13 — Can debugger traces measure retail cadence without qualification? → No; breakpoint stops perturb timing, so debugger traces are diagnostic only.` (evidence: `debugger collector semantics; scheduling report; method analysis in Section 4`)
- `[RESOLVED] E-OQ-14 — What is the selected method? → Bounded shared-memory instrumentation for state/order plus separate clean DXGI runs for cadence/pixels.` (evidence: `oracle collector inventory; Sections 4–6`)
- `[RESOLVED] E-OQ-15 — Is the native capture environment enrolled and ready? → No; doctor reports invalid tool lock, invalid native-capture build identity, and unenrolled capture.` (evidence: `oracle.py doctor at 2026-07-21T23:44:15.277369Z`)
- `[RESOLVED] E-OQ-16 — Does workspace READY mean runtime READY? → No; workspace READY covers link/package/git-marker/root separation only.` (evidence: `workspace-status --json compared with status/doctor`)
- `[RESOLVED] E-OQ-17 — Is fixture configuration currently owned? → No; the startup recipe explicitly reports fixture_configuration_owned=false and blocks sealed runtime without a configurator.` (evidence: `startup-lifecycle-recipe --include-back-reselection`)
- `[RESOLVED] E-OQ-18 — Is the existing MTNK recipe a reproducible fixture? → No; it does not seal exact map/state/object/command authority and no finalized scenario exists.` (evidence: `mtnk-empty-cell-move.recipe.v1.json; scenario-directory read`)
- `[RESOLVED] E-OQ-19 — Are the ten headline Phase-5 fixtures sufficient? → They are a minimum smoke set; every A–D acceptance comparison requires a discriminator or an explicit withheld claim.` (evidence: `readiness plan:267..290; final A–D reports and implementation contract`)
- `[RESOLVED] E-OQ-20 — What must RNG evidence contain? → Full logical Scenario state plus API count, raw advances, rejected candidates, and accepted values.` (evidence: `host contract; final D report; Section 7.5`)
- `[RESOLVED] E-OQ-21 — Is save/load universally mandatory for E? → It is conditional: mandatory if persistence changes or in-flight state support is claimed; otherwise that capability must be withheld.` (evidence: `RawTrack report; final C/D persistence findings`)
- `[RESOLVED] E-OQ-22 — Can one final-state snapshot prove the cutover? → No; ordered events and later-object-visible intermediate state are required.` (evidence: `final D report: lifecycle/order fixtures; readiness plan:281..283`)
- `[RESOLVED] E-OQ-23 — Can executable captures prove atomic old-caller removal? → No; source review must separately prove every old owner/caller is retired.` (evidence: `final D handoff; implementation contract production boundary`)
- `[RESOLVED] E-OQ-24 — Was runtime authorization available in this slice? → No; only the bounded read-only preflight report was authorized.` (evidence: `private AGENTS.md; task ownership boundary; readiness plan:388..394`)
- `[DEFERRED] E-OQ-25 — What exact command-dispatch hook safely emits CommandConsumed?` (category: `requires-different-system-context`; reason: `the older design marks the hook as not re-frozen and enrollment requires private-oracle design/review work`; next-step-if-pursued: `perform a bounded read-only native hook investigation, then review the hook/trampoline contract before implementation`)
- `[DEFERRED] E-OQ-26 — What minimal movement-hook set captures every ordered event without unacceptable hot-path overhead?` (category: `requires-different-system-context`; reason: `requires protocol and callback design in the private oracle repository`; next-step-if-pursued: `design compact event records and prove coverage against Sections 7–9 before adding hooks`)
- `[DEFERRED] E-OQ-27 — Does a generalized large transport preserve all movement callback constraints?` (category: `requires-different-system-context`; reason: `the existing large transport is startup-oriented and has no reviewed live movement producer`; next-step-if-pursued: `audit callback ownership/backpressure and implement offline tests before runtime enrollment`)
- `[DEFERRED] E-OQ-28 — Does the hidden-state observer leave state and pixels unchanged?` (category: `requires-different-system-context`; reason: `no movement observer exists, and pinned SyringeEx writes callback return to TEB FS:[0x14] after callback-local restoration`; next-step-if-pursued: `first add loader-side preservation or prove native non-consumption/equivalence, then seal observer-nonperturbation evidence using instrumented repeatability and clean presentation controls`)
- `[DEFERRED] E-OQ-29 — What are the exact realized GameSpeed throughput and jitter distributions?` (category: `requires-different-system-context`; reason: `requires separately approved clean retail runs`; next-step-if-pursued: `re-enroll DXGI, seal environment identity, and run clean repeats for GameSpeed 0..6`)
- `[DEFERRED] E-OQ-30 — Can the recipe configurator deterministically produce the AMCV/MTNK starting cells and LogicVector order?` (category: `requires-different-system-context`; reason: `create-checkpoint is a stub and fixture configuration is unowned`; next-step-if-pursued: `implement and review deterministic checkpoint creation with native command/state receipts`)
- `[DEFERRED] E-OQ-31 — What retail values will the movement captures produce?` (category: `requires-different-system-context`; reason: `no authorized retail run or evidence mutation occurred`; next-step-if-pursued: `execute the reviewed two-lane oracle after prechecks and approvals pass`)
- `[DEFERRED] E-OQ-32 — Do Detach listeners and BREAK receivers add unmodeled movement-visible effects?` (category: `requires-different-system-context`; reason: `final D deliberately leaves these research families open`; next-step-if-pursued: `complete the named focused lifecycle investigations before broad certification`)
- `[DEFERRED] E-OQ-33 — Are alternate DestroyOverlay, class-specific arrival, cache, and full visibility families equivalent?` (category: `bounded-cost-too-high`; reason: `these are explicitly bounded outside final D and cannot be inferred from the sampled E fixtures`; next-step-if-pursued: `split them into focused exhaustive investigations and add their acceptance fixtures`)
- `[DEFERRED] E-OQ-34 — Does a movement/lifecycle cutover preserve non-ground behavior?` (category: `requires-different-system-context`; reason: `shared lifecycle regression scope is outside this ground-only preflight`; next-step-if-pursued: `define and execute a separate non-ground regression matrix before shared-owner changes`)
- `[DEFERRED] E-OQ-35 — Can Rust consume and compare the final artifact schema without lossy normalization?` (category: `requires-different-system-context`; reason: `the retail schema and bundle do not yet exist`; next-step-if-pursued: `freeze schemas first, then implement adapter/comparator with rejection tests`)
- `[DEFERRED] E-OQ-36 — Is the production atomic cutover parity-certified?` (category: `out-of-scope`; reason: `Checkpoint E has no captures and static old-owner removal/design reconciliation remain separate gates`; next-step-if-pursued: `close E, resolve remaining D/design blockers, and cold-review the final cutover plan`)
- `[DEFERRED] E-OQ-37 — How will corrupt or synthetic speed-conversion boundaries be executed without weakening the read-only observer contract?` (category: `requires-different-system-context`; reason: `stock AMCV/MTNK runs cannot create NaN, infinity, invalid conversion, or arbitrary wide signed inputs`; next-step-if-pursued: `design and separately authorize a bounded native state/call producer, or explicitly withhold those runtime claims`)
- `[DEFERRED] E-OQ-38 — Can all four late session-end flags be distinguished in retail runtime evidence?` (category: `requires-different-system-context`; reason: `one natural terminal exit proves only the shared branch and synthetic flag mutation is not authorized`; next-step-if-pursued: `identify safe natural activation fixtures for each native test or withhold exhaustive four-flag runtime coverage`)

The deferred pile is intentional for a coverage-map. It is the execution queue, not
evidence that retail behavior was captured.

## 14. Adversarial Questions

1. **What if one ranged call consumes more than one raw RNG word?** The record must
   retain every rejected candidate and the complete logical state; an index delta is
   insufficient.
2. **What if a victim before the live cursor is removed?** The event stream must
   preserve vector count/index changes and prove which later object ran or skipped.
3. **What if the movement hook changes FPU state and therefore the result it records?**
   Callback review and observer non-perturbation must cover FPU/SIMD preservation;
   repeatability alone is not enough.
4. **What if the final coordinate matches but arrival occurred one object slot late?**
   The fixture fails unless ordered arrival and later-object visibility receipts match.
5. **What if a clean cadence run and instrumented state run choose different objects
   or maps?** They cannot be paired; fixture identity and command receipts must match
   the sealed manifest even though the lanes are separate.
6. **What if StateSnapshot fits the large slot but overflows the ring?** Capacity per
   record does not prove producer throughput; overflow is terminal evidence failure.
7. **What if pointer values differ across repeats?** Normalize only run-local pointer
   identities while preserving within-run equality, ordering, and reuse detection.
8. **What if a parked Hover changes vertical state without a target?** The mixed
   population fixture must record the actual Hover Process/vertical behavior instead
   of applying a target-based generic skip rule.

## 15. Implementation / Tooling Handoff

This is a future tooling handoff, not authorization and not a patch plan.

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| supported original-YR execution is absent | oracle status/registry | `run-original` is STUB | private oracle facade/runner | implement one reviewed, fail-closed original-YR execution path | dry schema tests, then separately approved sealed MTNK run | do not bypass `oracle.py` with ad-hoc scripts |
| checkpoint creation is absent | oracle status and recipe | recipe exists; finalized scenario absent | private checkpoint configurator | seal exact map/object/command/precondition identity | deterministic repeated checkpoint hash and native receipts | do not treat menu coordinates as state authority |
| command enqueue is not consumption proof | snapshot design | no runtime CommandConsumed record | command hook/protocol/collector | emit correlated native dispatch receipt | exactly one matching receipt; reject zero/duplicate/wrong object | do not infer from SendInput brackets |
| atomic movement state requires an in-process observer | snapshot design | codec only; no producer | instrument callbacks/protocol | publish bounded atomic state and event records | two deterministic instrumented repeats with no overflow | do not use sequential external reads as final oracle |
| current snapshot is partial/stale | codec versus A–D envelope | essential fields/order receipts missing | protocol schemas and producer | add exact semantic fields or compact typed events | golden encode/decode plus native field calibration | do not preserve stale mission labels as authority |
| 3,416-byte record fits large but not legacy transport | transport constants | kind 5 not routed through large live path | transport integration | reuse/generalize reviewed large transport or bounded records | max record round-trip, capacity/overflow/corruption rejection | do not claim fragmentation is inherently required |
| current manifests lack movement hooks | hook manifest | startup/tick only | hook investigation/manifests | enroll minimal verified movement/event hooks | byte identity, receiver, trampoline, register/FPU preservation smokes | do not install every probe spine address indiscriminately |
| hidden observer must be non-perturbing | snapshot design/startup contract; instrument README/callback shim | proof absent; known post-callback `FS:[0x14]` write remains | instrumentation certification and loader boundary | preserve/restore the TEB location at the loader boundary or prove native non-consumption/equivalence, then seal state/pixel non-perturbation evidence | machine-state/TEB preservation smoke plus repeatability, clean controls, and write-confinement audit | callback-local restoration cannot undo a loader write after return; do not equate repeatability with non-perturbation |
| cadence evidence must be clean | scheduling report | DXGI build unenrolled | native DXGI collector/environment | repair reviewed enrollment; run clean GameSpeed repeats | timestamped GameSpeed 0..6 distributions | do not use debugger/hook timing as retail cadence |
| fixture matrix exceeds ten smokes | A–D reports/plan | no fixture suite exists | scenario manifests/runner | encode Sections 7–8 with negative controls | every acceptance comparison maps to retail artifact or withheld claim | do not certify from AMCV/MTNK happy paths alone |
| runtime RNG evidence must be full | host/D reports | current schema has three RNGs but no call/event attribution | producer/event schema/comparator | record full state plus raw/API/rejection receipts | forced rejection fixture matches state and accepted result | do not use modulo index deltas as raw advance count |
| exact-once population includes Teleport | final C report | older design/contract wording is stale | public design/contract plus future runner | reconcile population and exercise idle/active mixed order | each eligible ground object runs once in native order | do not perform Drive-only production flip |
| lifecycle effects are intermediate evidence | final D report | no native event stream/comparator | lifecycle events and fixture schemas | preserve same-tick mutation/later-object visibility | D fixtures 1–21 emit exact ordered receipts | do not compare only end-of-tick hashes |
| save/load scope is conditional | final C/D/RawTrack reports | capability currently absent | scenario matrix/persistence policy | either capture supported in-flight state or explicitly withhold | first two post-load turns match when capability is claimed | do not call it optional while claiming full save parity |
| comparator authority must be retail-derived | project parity rules/status | comparator and VERA adapter absent | private comparator and public test adapter | consume immutable retail bundles with lossless normalization | corruption/identity/order mismatch rejection and exact match cases | do not generate goldens from Rust or hand calculations |
| production cutover remains blocked | implementation contract/final D/this report | no E evidence; old-owner review pending | public design/contract/production plan | close blockers and review atomic caller retirement | native artifacts plus static no-old-owner proof | do not add handled-ID or partial-population bridge |

### Required sequencing

1. Obtain sole ownership of the relevant private-oracle paths after the current
   input-provenance work is settled or explicitly partitioned.
2. Write and cold-review an oracle movement-capture design/implementation contract,
   including resolution of the post-callback `FS:[0x14]` write.
3. Repair the reviewed local tool lock and DXGI enrollment through the supported
   facade under separate filesystem-mutation approval.
4. Implement offline protocol, fixture, runner, and rejection tests without
   launching the game.
5. Perform a bounded read-only native hook investigation for command and movement
   probe safety; cold-review manifests/trampolines.
6. Request separate approval for hook enrollment/injection and for native input.
7. Execute instrumented state/order repeats.
8. Execute separate clean DXGI cadence/pixel repeats.
9. Seal/hash bundles and then implement the Rust adapter/comparator.
10. Reconcile the final design/contract and review the atomic production plan.

Steps 3, 6, 7, and 8 are separate safety decisions. Approval for planning or
offline implementation must not be interpreted as approval to launch, inject,
attach, send input, or mutate evidence.

## 16. Negative Facts and Withheld Claims

- No Checkpoint-E retail movement run occurred.
- No runtime debugger was started or attached.
- No movement hook was installed.
- No native input was sent.
- No runtime oracle evidence artifact was created, enrolled, replaced, finalized,
  or deleted.
- No native movement field value in this report is an observed runtime value.
- No actual GameSpeed throughput or jitter value was measured.
- No StateSnapshot kind-5 runtime record exists.
- No CommandConsumed runtime record exists.
- No finalized MTNK scenario exists.
- No AMCV, Walk, Hover, Ship, Teleport, Tube, miner, crush, Scatter, gate, factory,
  wall, bridge, cache, or delete fixture exists in the oracle checkpoint store.
- The existing codec/golden proves serialization structure only.
- The existing transport smokes prove transport infrastructure only.
- The existing cloned Rust host tests are regression/contract evidence only.
- No parity status was upgraded to VERIFIED.
- Production movement authority must not change from this report.

## 17. Final Checkpoint Statement

Checkpoint E is **BLOCKED / PRECHECK_FAILED**, not failed evidence and not a parity
disproof. The required retail experiment has not yet become executable through the
reviewed oracle interface.

The highest-leverage next step is a reviewed private-oracle movement-capture design
and bounded offline implementation plan that:

1. seals one exact MTNK fixture;
2. reuses or generalizes the existing 4,096-byte transport correctly;
3. defines compact ordered movement/lifecycle events plus bounded snapshots;
4. adds native command-consumption authority;
5. separates instrumented state runs from clean DXGI timing/pixel runs;
6. expands systematically to the full discriminator matrix.

No production Rust work is justified until executable retail artifacts exist and
the remaining D/design/static-caller blockers are reconciled.

## Sources

### Public movement evidence

- `docs/plans/2026-07-20-ground-movement-atomic-flip-readiness-investigation-plan.md` — SHA-256 `4C6E3B77FBC26D014A52EB115B33D7A2C6DFC5F84BECE48743D5FCEB0544884B`.
- `docs/contracts/2026-07-20-ground-drive-process-track-stepping-implementation-contract.md` — SHA-256 `24F0ABB804EBF0A7A3EA860F832285A1F6FB44CEFEAE5FF1FC385B3623D0907B`.
- [docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OBJECT_PASS_DRIVE_INVOCATION_SCHEDULING_GHIDRA_REPORT.md) — SHA-256 `5A9E6CB3DE67E3637C001A42EC6C7D34FEFD2AEDA097EFD82BBBCB388038C263`.
- [docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TECHNO_MISSION_MOVE_FOOT_LOCOMOTOR_HOST_CONTRACT_GHIDRA_REPORT.md) — SHA-256 `4D85178F0EF454AA34472537EF8FA33DB501026C6703897BA1D4A91EB990FD63`.
- `docs/research/FOOTCLASS_GET_CURRENT_SPEED_EXACT_GHIDRA_REPORT.md` — SHA-256 `0A728B262FA8358C6FDE931C93216EC5C7378D51EDC1A07BBD38FBFD4E689683`.
- [docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DRIVE_RAWTRACK_METADATA_INITIALIZER_RECONCILIATION_GHIDRA_REPORT.md) — SHA-256 `3B94CF7E896B058CA1ECEBAB69CA63D0B736D7C46AD5D35B137FD6934CCCC93E`.
- [docs/research/GROUND_PHASE1_LOCOMOTOR_POPULATION_AND_PRECEDENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_PHASE1_LOCOMOTOR_POPULATION_AND_PRECEDENCE_GHIDRA_REPORT.md) — SHA-256 `CBE8307F6AF27760A151D0A599C5D7400727840E3C6C2195FFA1598E82ADE37D`.
- [docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GROUND_MOVEMENT_LIFECYCLE_EFFECT_OWNERSHIP_GHIDRA_REPORT.md) — SHA-256 `A4E6DF032FE11EE5E2A2D96399624AE5B19418DFF1E3C1BB683C4DD2ECF765FF`.
- `docs/contracts/2026-07-12-oracle-native-startup-authority-common-initial-state-implementation-contract.md` — SHA-256 `0A0FE22934814A9A7F331E14B556FB0C4666E7D682239961FCFF243B04DC4FBE`.

### Private oracle evidence, read-only

- Private repository HEAD `7b8689edd2c5a26ec936caaa03d2c7c9bc31523e`.
- `AGENTS.md` — SHA-256 `ADD2A0591527A3DBC57A33AAFBE297230EC4F7DC9923BCD1F40AC01D6D97EB90`.
- `tools/oracle_harness/oracle.py` — SHA-256 `F305405603860635720E6EB8E9FBF46B7FB9C6EB4C147BB6858C562DADF21CDB`.
- `tools/oracle_harness/oracle-system.v1.json` — SHA-256 `3088F0010AA52A83BF7087C68ADA1EC8F8F17593C7888628FC28972E6D45BFF7`.
- `tools/oracle_harness/tool-spec.v1.json` — SHA-256 `0CB86F8F0DB9891CABF63C7F145CB7414962CEBCD1003B1DEA2BB70C9E5D162C`.
- `tools/oracle_harness/oracle_harness/cli.py` — SHA-256 `E20E8C93E9DEC25FFA388BBC4053401EA766335021FC66294A046334C0576B54`.
- `tools/oracle_harness/oracle_harness/collectors/debugger_mcp.py` — SHA-256 `69249DF3AE369AD1365B19AB850070C13B17D19FEC51AE5A1E3F75E6EAD4E487`.
- `tools/oracle_harness/oracle_harness/collectors/debugger_backend.py` — SHA-256 `45C36590BA1D2EF207CBC90D16E0CF1CC8FBBF98CD3C0AE02AE68D8857E00F3E`.
- `tools/oracle_harness/oracle_harness/collectors/oracle_instrument.py` — SHA-256 `262403A595BBA74316DD972BD0589D8B79ED86A709B35F2F7B792C3F1C33774D`.
- `tools/oracle_harness/oracle_harness/collectors/native_dxgi_capture.py` — SHA-256 `48D9CCBFEF4A37C970C24B5C0C415243B692CD969E03456342A674E1962EADC8`.
- `tools/oracle_harness/scenarios/mtnk-empty-cell-move.recipe.v1.json` — SHA-256 `E2F8A67A54C4C2297D4C188D30068AC7F121EAB369C1E3658F8752B05580B7C6`.
- `tools/oracle_protocol/README.md` — SHA-256 `C52D9CE00AA6E9FF23F24C83EE0DBD190AD1F55C12CCAFA5C11EC6160A09DC15`.
- `tools/oracle_protocol/src/snapshot_v2.rs` — SHA-256 `B54B5C2DD66C77DB55E2913B7349B3283A167CA3332999C0CCBC80F2AD4227C4`.
- `tools/oracle_instrument/src/transport.rs` — SHA-256 `271E1E49FC92B6C5E42716BC8C14E6AC819A0C04757345A30DC155FF2C480511`.
- `tools/oracle_instrument/src/startup_transport.rs` — SHA-256 `751F0ED61565B8C8AEC2344F7DC1DF8475A3944D0B499109F4E80696B5D78FC0`.
- `tools/oracle_instrument/src/callback_shim.rs` — SHA-256 `8A416C3757517DAF22CF0BBE8ED6684C2ED3D64D5E3BAB38BFA16455F8B1F428`.
- `tools/oracle_instrument/src/lib.rs` — SHA-256 `856E0815740DA0242B0495418653B788A622ACB0238FDC7F2946B85ACF669CFE`.
- `tools/oracle_instrument/README.md` — SHA-256 `17DD66251868643F89ECC03203FB3E5D725A94864B13FA8EE0953D4892789DAC`.
- `tools/oracle_hook_manifest/manifests/gamemd-retail-1cdd1180-syringeex-v0.1.0.2-source-v142-sdk19041-startup-observed.hook-manifest.v1.json` — SHA-256 `7DCAD2C01B7492B6EA94A0909B62644D23BB3F4989559FFD0D05A9E39DBABC8E`.
- `docs/plans/2026-07-12-oracle-rng-mtnk-state-snapshot-design.md` — SHA-256 `758B3044274F12B444FDBB63B1511FF4F9EFEE24B6E8E4B3D061119F9D286B0C`.
- `docs/plans/2026-07-20-stage-13b-primary-certification-plan.md` — clean-HEAD blob `818314c6c8174f4401699bf63226389067a445ee`, SHA-256 `21764D15B44CE44F0522FD76D1446E8227E746AB85675834841E94B8F0F0A25C`; later companion WIP SHA-256 `A8A139F6DB7E0EA11040CB523858C1FFB5D953671E02167ED357C3DA71162BDB` was excluded.
- `docs/research/LOGGER_DLL_TWO_HOOK_RUNTIME_ABI_SMOKE_REPORT.md` — SHA-256 `7F684FA74961488A5B86B7DF54F3FCA62A6C322C4636906E70AED0C2C649C347`.
- `docs/research/LOGGER_DLL_RUNTIME_OBSERVATION_TRANSPORT_REPORT.md` — SHA-256 `13743569D0E25D2EDF0595B970B0EA309F4F1F47329205E7558D9D3261CD34C2`.
- `docs/research/DAMAGE_ORACLE_CAPTURE_CONTRACT_2026-07-13.md` — SHA-256 `D1D483294A18B6A603587AD8BE7195F5453EA5EB5D416755DCE13FB43F686FCC`.

### Retail fixture and environment evidence, read-only

- `gamemd.exe` — SHA-256 `1CDD1180E49024FBDA8AD568CAAC2E86E856063FF67AB38F62B7D2C7BB84298C`, 5,286,504 bytes.
- `ddraw.dll` — SHA-256 `2F1399EF5E6CDBA02495FBB66C731924C9A7B0B40B43B8B496DFB18993328039`.
- `DDrawCompat.ini` — SHA-256 `AB96FDEFE27D7E6E185AD7AA50AB94B903C8C742CAE38425BA63D6FA6E76300E`.
- `RA2MD.INI` — original-preflight SHA-256 `466DF459C8464700EE4B9B5CAA8739D9D3BCA35B26C5A6AEB1E0B1C1E76F1BB6`; final-review SHA-256 `792ABE03D8A5B67E02A758E8ACB1B643583CB4C79DA2C566553478A74855549F` (file mtime `2026-07-22 21:47:00 +02:00`, observed `2026-07-22 22:13:05 +02:00`); mutable user state, recorded as environment only and never fixture authority.
- `DeepFrze.yro` — SHA-256 `1904D7337ED2EAE8AC321E2B31FBBE6B7E3117DD8C99C3C911DBED09931F8B8A`, 239,496 bytes.
- `CrctBrd.yro` — SHA-256 `039EB0A5F40AAF33AB3B308123B84ED277456A2846E9F527D8DE32EB5728A58F`, 188,536 bytes.
- `IrvineCa.yro` — SHA-256 `086DEF8929B6E65E9E13704E908DE189BB189C1BD9BB27D3B99E2F3AD2867767`, 218,792 bytes.
- `Dustbowl.map` — SHA-256 `46B07F8968BE4C267CBDEC5B99CF36E9BDE98F4AC0D23B7D634ABF86E9165A79`, 125,288 bytes.
- `Carville.mmx` — SHA-256 `3268FD2DD00E0D572497F3CE1540B204929232D92E1A21EEC8179BE6D6A74796`, 146,616 bytes.
- `ini/rulesmd.ini:390,6603..6648,11722..11744,12515..12538` — SHA-256 `3D341EF8A13A4B5AB24AF2EEF48AC94931AC2BB87D950FE3330A07E2D25672EF`; stock BaseUnit, MTNK, and refinery/free-unit data.

### Read-only commands executed

- `python tools/oracle_harness/oracle.py capabilities --json`
- `python tools/oracle_harness/oracle.py status --json`
- `python tools/oracle_harness/oracle.py workspace-status --json --retail-root <retail-root>`
- `python tools/oracle_harness/oracle.py startup-lifecycle-recipe --include-back-reselection`
- `python tools/oracle_harness/oracle.py doctor`
- `git rev-parse HEAD`, `git status --porcelain=v1 -uall`, `Get-FileHash`,
  `Get-Item`, `Get-ChildItem`, `rg`, and direct file reads.

No Ghidra runtime command was needed for this tooling preflight; all binary anchors
are inherited from the frozen A–D reports and remain subject to hook-enrollment
revalidation.
