# Engine authority and migration audit

Objective: eliminate redundant state, competing behavior, obsolete call chains and
incomplete migrations throughout VERA20k. A completed increment does not complete
the whole-engine objective. Baseline: `0ceb85d3` (origin/main, 2026-09-20).

## Acceptance established before implementation

For each mechanism:

1. Identify its authoritative state, all writers, readers, production entry points,
   lifecycle transitions, save/restore and deterministic hash paths from current source.
2. Classify parallel representations as harmful duplication, native distinctions,
   derived caches, immutable inputs or adapters. Retention requires a concrete reason
   and independent review, including invalidation and serialization when applicable.
3. Specify observable behavior and ordering that must remain unchanged. Resolve
   conflicting implementations against native bodies/callers/data; identify behavior
   corrections separately from structural changes. Do not invent native equivalence.
4. Migrate all affected consumers to the intended owner and remove superseded state,
   APIs and code. A moved body or unused shared helper is not a completed migration.
5. Validate through the real frame/command/load paths affected by the change, including
   deterministic continuation and snapshot compatibility where state changes. Preserve
   math widths/evaluation order, scheduler/RNG order and supported platform builds.
6. Describe the concrete maintenance benefit. Run focused checks and, for the final
   Rust candidate, the full library tests and library Clippy required by AGENTS.md.
7. Obtain fresh independent read-only criticism of requirements, original evidence,
   design, full diff and literal validation. Resolve confirmed findings and re-review
   until passed. Update documentation and independently confirmed Ghidra annotations
   when warranted, then commit, publish and merge the validated increment.

Final acceptance additionally requires a whole-engine audit accounting for every
identified duplication/migration, including independently reviewed retained copies.
Open candidates or merely recorded cleanup remain unfinished work.

## Coverage ledger

| Area | Current inspection | Disposition |
| --- | --- | --- |
| Simulation storage, lifecycle, frame scheduling, combat, economy, AI | Read-only audit in progress | Open |
| Map, cells, navigation, movement and locomotor migrations | Owner tracing in progress | Open |
| Application state, loading, persistence, net/replay | Read-only audit in progress | Open |
| Rendering, sidebar/UI, audio | Read-only audit in progress | Open |
| Rules, assets, shared utilities and binary entry points | Initial module inventory | Open |

## PR415 readiness review — 2026-09-20

The user requested resolution of the published draft's merge blockers. This is a
bounded delivery of the accumulated engine-ownership work, not completion of the
whole-engine objective above. The candidate is on `feature/track-state-authority`;
it includes `origin/main` at `608eabf4`. The separate main checkout was not changed.

Implemented areas:

- Signed 32-bit actual health and live Type Strength replace retained maximum
  health; estimated health remains a separate native state owner.
- Foot current/navigation coordinates, locomotor-specific physical height and
  teleport restoration; Drive/Ship track admission, continuation and full 16-bit
  fresh heading comparison with same-turn Facing publication.
- Dock slots are serialized authority, with a rebuilt reverse index and `u32`
  pad indices through consumers, save and hash paths.
- Building damaged state, 21 retained animation slots, bounded operational and
  storage integration, authored NeedsEngineer shutdown and changed-owner resume,
  and InfantryAbsorb slot selection.
- Shared Building/Tile animation conversion bounds are explicit match/save/replay
  inputs. The user selected this compatibility policy. Snapshot/hash schema 171
  records those bounds and HasEngineer. The fixed rational conversion deliberately
  omits native intermediate float32 rounding, under AGENTS.md's numeric policy.
- Existing formation speed caps continue to limit live type speed. Fly height
  feedback follows XY movement onto the destination terrain, allowing actual
  landing there while preserving exact Z and save/restore continuation.

### Replay regression disposition

All three baseline fixtures pass at `bd5928e6`. Frame-by-frame comparison covers
17 Slice6, 201 bridge and 601 global states. Complete states of all three RNGs
match at every captured frame; health values also match. Bridge positions are
identical throughout. The intentional movement differences come from native
fresh-turn admission, retained residual clearing and live type acceleration
replacing a stale retask cache. Timer and target-speed publication changes are
separately attributed. Historical pre-health-migration hashes remain archived
receipts; they cannot be reconstructed by the current signed-health fold.

See [track regression notes](../research/TRACK_PROCESS_REPLAY_REGRESSION_NOTES.md)
for causal evidence, coverage and original executable harnesses. The global
fixture also continues 16 frames beyond its old terminal boundary to check that
an intermediate retired track resumes. These are Rust regression baselines, not
native whole-scenario goldens.

### Final validation

- `cargo test -p vera20k --lib`: **9,145 passed, 0 failed, 134 ignored**,
  final untraced run (`.local/pr415-blockers-library-04.log`). All three replay
  fixtures and the new production regressions pass.
- `cargo clippy -p vera20k --lib`: **passed**, with 1,215 warnings
  (`.local/pr415-blockers-clippy-02.log`). The first normal build exposed a stray
  `cfg(test)` on an infantry rendering helper; removing that attribute restores
  its production caller without changing the already-tested function body.
- The original `drive_fresh_turn`, `track_speed_native`,
  `track_outer_entry_continuation` and `building_pixel_coordinates` corpora replay
  successfully within their declared coverage.
- Fresh independent read-only review accepted the actual final fixture gates,
  causal attribution, Fly, formation speed, shared coordinates, Building slot
  policies, docking, presentation consumers and the normal-build inclusion fix,
  using the actual passing full-suite result.

Validation is local Windows. GitHub's CI workflow is manually disabled and has
produced no run for this branch; no Linux/macOS execution is claimed. No repository
CI configuration was changed. These results establish readiness for this bounded
PR, not completion of the whole-engine coverage ledger.

### Scope and remaining work

The coverage ledger above remains open. Adjacent unfinished migrations are not
represented as completed by this PR:

- Foot idle/destination and scanner callbacks, DistributedFire, estimated-health
  producers/debits and persistent Facing ownership.
- House aggregate storage, ordered Building registration, shared spending and
  affordability, sale ordering, raw IncomeMult and destruction scattering.
- General Building body/timer ownership, Super/turret and constructor postlude,
  HasPower/EMP/overpower/temporal producers. The existing shared cargo model does
  not independently represent absorbed and garrisoned occupants for hybrid types.
- Signed Infantry action records and AIAutoDeployFrameDelay are parser groundwork;
  native pending-Stop/DoAction/stage/reload production migration is not implemented.
- Arbitrary native saved Tactical matrices and historical snapshot import are not
  supported. Version 171 rejects older layouts; restored-map/process prerequisites
  remain explicit. The native replay format has no custom bounds extension.

[Building admission and coordinate policy](../research/BUILDING_SLOT_ADMISSION_AND_COORDINATES.md)
describes native identities and intentional limits. Local diagnostic logs and
retail binaries are excluded. Native corpus coverage does not establish universal
native equivalence; Windows validation does not claim Linux/macOS execution.

The sections below are historical mechanism notes. Their intermediate test counts,
status and proposed next steps are superseded by this readiness review.

## Increment 1: one house wallet

Chosen authority: `HouseState.economy.credits`. Remove `HouseState.credits` and
the factory's four copy-in/copy-out adapters. Initialize the sole balance in
`HouseState::new`; migrate every direct house consumer and the production credit
accessors, preserving each caller's existing arithmetic and missing-house policy.

Evidence at baseline: factory step (`production/factory.rs:1148`) and prerequisite
pruning (`:959`), cancel-last (`factory_lifecycle.rs:128`) and cancel-one (`:169`)
take the whole economy and temporarily overwrite its balance. The value left there
is serialized but `world_hash.rs:921` hashes the separate house balance. Between
factory calls, income writers change only the house balance. There is no useful
independent meaning for the retained factory balance.

Acceptance specific to this increment:

- No duplicate house balance or copy adapter remains, including tests and diagnostics.
- A production step consumes income written earlier to the same wallet; cancellation
  returns funds immediately visible to the sidebar, income and affordability readers.
- The hash folds the same balance at its existing position, once. The purifier-count
  and statistic folds remain unchanged in this increment.
- Snapshot version advances from 165 to 166 because a serialized field is removed.
  Old layouts are rejected before decode; current save/load retains balance and
  statistics and resumes production identically through the real load/frame paths.
- Initial funds, absent-house behavior, command scheduling and existing per-call
  arithmetic are preserved. `Economy::spend` and `credit_income::spend_money` have
  different existing statistics and negative-value semantics. Saturating refunds,
  wrapping native income and the other income/debit paths need a subsequent
  native-backed behavior increment; this structural change does not certify them.

Concrete benefit: one stored balance removes four synchronization obligations and
the ability for serialized wallet state to disagree with the value read by gameplay.

## Increment 2 acceptance: one Drive/Ship track authority (in progress)

Baseline: merged wallet PR #407, `5777c115`. Branch `feature/track-state-authority`.
Both ordinary and forced tracks must store selector, signed next-to-consume cursor,
short selection and residual only in the retained class `TrackProgress`. Absolute
head coordinates belong to Drive/Ship, while immutable tables supply geometry.
Remove `GameEntity.drive_track`, `forced_drive_track`, both detached runtime types,
and the superseded stepping/terminal/cell-crossing implementation. Test fixtures
must exercise the production host rather than preserve a second executable model.

Forced Drive admission must publish selector/cursor before native coordinate guards,
preserve residual/short selection, and publish full XYZ and occupation in native order.
It must not write owner applied speed; the proven bunker caller sets that separately.
Ship retains its distinct 64-selector table contract and has no forced admission.
Existing independent native TrackProcess witnesses remain the arithmetic/callback
contract; add a reproducible Force_Track witness before claiming those admission facts.

All retained active tracks must reach the same object-turn TrackProcess host without
requiring a MovementTarget. Compute speed on each visit; do not cache forced speed.
Migrate fresh/chain admission, cancellation, readiness, occupation, locomotor rollback,
serialization and hashing. Preserve native distinctions among head, destination,
track-valid, owner speed, occupation and call-local budget. Callback early exits must
keep their native no-writeback behavior; callback replacement must survive terminal work.

Validate actual object turns with first-point, multiple-point, residual-only and paid
sentinel visits for ordinary Drive/Ship and bunker selectors 0x43..0x47. Cover live
speed changes, crossing/list/raw-mark order, full head Z, slopes/bridge layers,
cancellation/replacement, callback lifecycle exits and save/load continuation before
point zero, midcurve and terminal. Assert no duplicate sensor/MCV/mission/RNG work.
Coordinate a snapshot bump from166 when fields are removed. Fresh independent criticism
must inspect native evidence, complete consumer migration and actual test results.

The full live speed getter and shared host receivers still have documented gaps.
Trace prerequisites exercised by these production paths and complete affected work;
do not use missing receivers or test-only execution to make the acceptance pass.
The refinery-interruption use of selector0x47 remains an unproven caller policy and
must be resolved against its native bunker-link guards, not silently blessed by reuse.

Concrete benefit: one stored track and one world executor remove cursor/residual
synchronization, geometry caches and the need to fix cell/lifecycle behavior twice.

Current evidence and corrections (not acceptance completion):

- The reproducible Force corpus now contains254 native rows; four Rust
  comparisons pass across246 covered cases (192 full no-overlay Force calls,
  24 supplied post-crate continuations,12 bunker-install tails,18 special-speed
  prefixes). Supplied callbacks do not implement actual crate pickup.
- The handoff mark reloads callback-mutated retained head XY; the final mark
  uses the original supplied XYZ. Native original instructions establish this
  distinction, and the new comparison cases exercise it.
- Process_Track admits code2 through its own jump table; the removed fresh
  Process_Movement gate was a competing, incorrect implementation. Crush
  admission is code0 and leaves killing to PerCell. The mixed independent
  vehicle-bit tail still needs complete migration to raw occupation inputs.
- Native terminal4B22AF goes to residual writeback4B1F5C and returns at4B25F9;
  it does not re-enter fresh track/tube selection in that same Process. Old
  tests expecting an immediate fresh turn or tube handoff are being corrected.
- Native release selectors belong to the reciprocal Bunker link, not refinery
  on-pad/contact bookkeeping. The refinery Force/speed policy is removed.
  Sale radio0x17 and death/contact-gone Unload have different native receivers;
  the eager shared refinery reset does not establish equivalence. Bunker release
  now uses the Power_On/Force/speed/link/BREAK sequence. Installation deselects
  the live unit rather than concealing it: original Unit+150=5F44A0 does not
  change LogicVector, Mark or Limbo. Control-group clearing, the normal-release
  destination receiver and nearest-passable search still need complete migration.
- Current hash omits deleted payloads. Historical test-only projections retain
  the old absence markers only within an explicitly checked track-free bound;
  active obsolete payloads cannot generally be reconstructed from new state.
- The Unit post-latch tail uses the selected raw vehicle byte and the first
  GROUND Unit, independently of ignored/self identity and the active object-list
  layer. `cell_entry_crush_tail` reproduces68 native continuation cases;64 have
  matching Rust wrapper comparisons awaiting the current diagnostic. Nonzero
  accumulated codes survive. The preceding object walk remains separately bounded.
- Fresh selection still uses `DriveCellAdmission`'s owner snapshot/identity grid
  instead of the central live CanEnter classifier. With the old crossing executor
  removed, that omission has no later crossing check. Native flag8 turns also
  query a second coordinate. Completing this consumer migration is required;
  the deliberately different fresh and chain return-code policies must remain.
- Native speed inspection requires binary64 retained class/Foot fractions and
  live staged getter inputs, not fixed-point mirrors or a cached readiness result.
  Target publication belongs to ProcessMovement admission, while TrackProcess
  consumes that retained value. Full XYZ braking, Passive, sinking/crush flag
  producers, house/crate/CTF inputs and exact numeric rules are required. The new
  `track_speed_native` corpus reproduces69 getter and116 prefix cases, but its
  pure numeric implementation is not yet connected to the production owners.

Current production consolidation also removes command-time Drive/Ship track
preparation. Move orders retain the destination and route; Process owns the
first fresh selector/head, queue shift and occupation. This is a behavior
correction: the removed command path bypassed admission altogether. Existing
in-flight heads remain independent of replacement destinations. Tests must
check no head/claim at order time and actual admission on the first Process,
including zero-budget and save/restore boundaries.

ProcessMovement now publishes the class target separately; active TrackProcess
consumes the retained target for both families. The Ship-only post-Stop override
is removed. Passive and signed selector>=64 gates preserve the native distinction
from Accelerates=false. Full native precision and the live getter migration are
still required. The current fresh publication follows successful preparation,
so refusal/turning cannot run the speed prefix. Completing the world continuation
must place target publication after the first successful query and before the
entering callback, preserving callback-visible state.

Walk placement/path-search and Drive/Ship placement now use one Foot Mark/list
corridor, preserving mark-byte/list/callback/raw/Recalc ordering and concrete
Unit versus Infantry raw receivers. Discovery/tag4 remains an explicit missing
receiver. The shared track-entry classifier now keeps terrain wall codes4/5
and merges live ordered occupancy/raw/frame results; chain supplies real wall
and type context. Fresh admission still uses the obsolete DriveCellAdmission
snapshot and must migrate before this increment can pass.

The retained Process admission now uses each class's valid byte and selector,
independently of head presence. Ship+63 was missing from Rust and is now retained,
published during fresh/chain acceptance and cleared at the native terminal and
callback boundaries. The historical schema166 test projection omits this newly
represented Ship byte; current schema167 serialization/hash retains it. A real
object-turn fixture varies valid, selector and null/non-null head independently.
The new original-entry witness covers144 Drive/Ship cases with original type
getters and early residual stores.48 non-tube/non-turn-latch cases compare against
the production retained-admission predicate. Full +62 latch/tube8 entry behavior
and early residual reset remain required, not certified by that subset.

Chain now invokes one Simulation::query_track_entry world owner, which retains
explicit effective-height arguments and split layers and rebuilds live type/wall/
raw/object facts per query. Fresh still must migrate to this owner. A separate
pure fresh dispatcher compares100 native first/second query continuations plus16
normalization cases; it is not yet connected to world response execution.

The no-op process_drive_locomotion_shell/DriveProcessOutcome corridor is removed.
Its only production call discarded a presence-only result for every collected
mover. The historical host trace now names its component-presence observation
explicitly; real Process remains covered by object-turn tests.

Fresh entering callback prerequisites are now concrete: Unit7416A0 entering=true
requires Crusher or ability0x11, exact plane lookup and low5 infantry bits, then
CellScatter(NULL,true,false,plane). Existing forced hut and single-blocker helpers
cannot provide its live eligibility, all-recipient snapshot, synchronous class
callbacks or RNG order. Native vector growth is10, not a10-recipient cap. Do not
reuse that erroneous cap. Drive/Ship+62 is a Process-latched turn byte, independent
of the live Facing query; clearing it invokes Unit PerCell(reason0), distinct from
the track terminal's reason2. The existing MCV-only `mcv_drive_was_rotating`
and its `drive_process_prelude` already duplicate part of that lifecycle; move
their producer/consumer behavior into the shared class owner and remove the
adapter field/call chain. These are required migration work, not exclusions.

Post-arrival rebuilding and the generic cell-transition helper no longer create
Drive/Ship tracks. The latter cannot accept a class payload or Foot queue, so
it cannot become a competing admission owner. Tests formerly invoking that
unreachable constructor now enter actual Process. The persistent
`pending_track_occupation` field is removed from both classes: fresh acceptance
hands its Apply1 obligation to the synchronous world host on `TrackInvocation`.
No save/frame boundary exists inside that handoff. Retained/forced invocations
cannot infer fresh admission from cursor zero and do not repeat Apply1.
Schema167 remains the current uncommitted migration; bounded test-only schema166
hash projection retains the former false field in its original derived layout.

The native `track_fresh_admission` harness now reproduces148 original caller
continuations across Drive/Ship: first and second CanEnter result dispatch,
Mark/query ordering, caller coercions, recursive code2 arguments and distinct
chain eligibility. Mark/CanEnter returns are explicitly supplied external calls;
these cases do not establish whole-process, classification or Mark-effect parity.

Full library diagnostic6 compiled and ran:9007 passed,14 failed,134 ignored.
Failures prompted migration of command-time fixture claims, missing chain map
geometry and target publication cadence. Its bounded bridge schema166 receipt
matched exactly before the current-schema pin; replay/RNG checks also passed.
Diagnostic7 finished9013 passed,8 failed,134 ignored. The pending-state removal
and admitted command fixtures pass. Two speed fixtures and one interner setup
needed correction; diagnostic8 then passed all95 movement tests. Group-movement
minimum spacing and dense position sequence
remain failing production checks; fresh admission and speed work stay required.
The corrected fresh native witness includes the second-candidate stack producer
and passed regeneration/check for148 rows: the subsequent Crusher overlay query
uses candidate2. TrackProcess entry has its own valid/selector-or-queue8 guard
before the speed prefix; reaching its outer CALL alone does not prove a speed
update on refusal or turning. Exact entry guard/early residual behavior remains
part of the required production migration. Current-schema
replay pins remain unchanged while required production work is unfinished.
Diagnostic9 compiled the current shared query, Ship valid-state and dispatch
changes:9020 passed,6 failed,134 ignored. All new bounded native comparisons
and valid/head independence checks passed. One terminal piggyback fixture had
replaced its accepted class with default valid=false; corrected to seed true,
and focused diagnostic10 passed1 test. Remaining5 failures are unchanged:3 current
replay hash pins, the dense position fingerprint and group minimum spacing.
No current full-suite pass, Clippy result, critic pass or publication exists.
Native speed/getter and crate/discovery/refinery receivers remain open.


Additional confirmed unfinished candidates: the economy purifier-count mirror has
no gameplay reader but adds a full entity sweep; animation execution remains split
between AnimClass, WorldEffect and app building animation; radar events and camera
history remain split between the simulation legacy queue and client type-5 queue.
Each remains open under the full objective, pending complete traces and migrations.

## Discovery ledger (not completion evidence)

Independent read-only source audits identified these open mechanisms at the baseline.
Line numbers are discovery coordinates; recheck current bodies before implementation.

| Mechanism | Producers / consumers establishing duplication | Required migration |
| --- | --- | --- |
| Economy purifier mirror | `world/mod.rs:4059` scans entities; `world_hash.rs:926` reads the serialized count; gameplay deposits instead call `miner_system.rs:3028,3071` | Remove unused count and sweep; account for hash provenance without retaining dead runtime state |
| Refinery contacts | `miner_dock.rs:41` contact maps and `game_entity.rs:621` radio state; `miner_dock_sequence.rs:1013,1067` writes both; snapshot cleanup and hash retain both | Unify contact admission/entered/cancel/expiry/restore; distinguish physical on-pad occupancy |
| Legacy resource nodes | `world/mod.rs:3397` chooses fallback; `ore_growth.rs:1972` and `terrain_spawn.rs:470` mutate node amounts alongside OverlayGrid | Establish production initialization requirements, migrate fixtures/consumers and remove superseded node storage/algorithms |
| Animation execution | `components.rs:834,896,987`, `anim_class.rs:367`, `world/mod.rs:5803`, `app/presentation/building_anim.rs:736`, `instances/overlays.rs:246` | Trace native construction for teleport/bridge/SW/muzzle/building producers, unify lifecycle and render consumers, fix save/restore ownership |
| Radar/history | `render/radar_events.rs`, `sim/radar.rs:216`, `minimap.rs:604`, `input/dispatch.rs:1492` | One event engine and history, migrate admission/EVA/draw/reset together; type-5 history currently masks later legacy history |
| Shell pointer mirrors | `app/shell_main_menu.rs:651` copies controller press/hover; main/single-player renderers read copies; transitions clear separately | Render from dialog-scoped controller, migrate reset/modal behavior, delete duplicated fields/handlers |
| Sidebar scroll | `presentation/state.rs:152` scalar plus per-tab rows; `input/dispatch.rs:1044` park/restore; wheel/gadget/projection mutate | One per-tab row owner, migrate all mutation/clamp/reset and retain immutable output projection |
| Audio initialization | `app/initialize.rs:266` and `loading/transitions.rs:433` reload process indices | Establish asset-reload/precedence semantics, then give initialization lifecycle one owner |
| Drive/Ship tracks | `track_host.rs:144` retained class tracks; `movement_tick.rs:65` forced old DriveTrackState; `drive_track.rs:4182` uses serialized before_first_point omitted by `world_hash.rs:347` | Unify forced stepping/markers/cursor/residual through intended class owner; fix behavioral-state hash omission, preserving native call-local vs retained budget |
| Walk readiness | `ready_producer.rs:316` reads generic movement target/phase while `locomotor.rs:481,520` exposes retained moving/head; consumed by mission and MoveSound | Migrate readiness and piggyback-END consumers before removing superseded phase; preserve destination/head/moving/animation bytes |
| Object Z | `combat/mod.rs:1650`, `render/locomotor_visual.rs:108`, `ground_pose.rs:65` reconstruct nonexact coordinates differently | Migrate remaining Fly/Rocket/Parachute/raw coordinate producers, then remove competing absolute-coordinate reconstruction; keep range/art/terrain offsets distinct |
| Jumpjet/common state | `jumpjet_movement.rs:506` old test-only helpers; `world/mod.rs:3314` still reads common crash-speed/derived phase for MoveSound; common capture/install retains unused state | Migrate final live gate with native evidence, remove superseded fields/helpers through snapshot/hash/piggyback; retain live walk fallback |
| Movement without runtime | `snapshot_mover` admits a generic no-locomotor integration path | Prove constructor/load invariants for move-admitted entities, migrate fixtures, remove alternate integrator; static/unknown/zero-speed absence is not itself a defect |

Reviewed retention hypotheses from discovery (require final independent verification):
cash vs spending/harvesting statistics; factory unpaid obligation vs house balance;
factory insertion order vs LogicVector order; real purifier count vs AI virtual bonus;
radio contact vs pad occupancy; mission selector vs handler runtime; immutable
SidebarView; private ScenarioCatalog projections; native rule construction order vs
typed RuleSet views; logical audio state vs physical playback; render overlay ordering
vs live identity; save-list embedded time vs quickload filesystem time; staged network
commands vs due commands and checksum history. These are distinct responsibilities,
not a blanket exemption for every field in those systems.

### Process turn authority acceptance (in progress)

Replace the MCV-only previous-rotation field and prelude with one retained +62
field on each Drive/Ship class. The actual ground Process corridor samples live
Facing only on the native non-track branch; an active TrackProcess call bypasses
that sampler. Completion clears the latch before PerCell(reason0), then reloads
alive/limbo/falling. TrackProcess must consume the retained latch, not live Facing,
and reject before speed/point work when latch is set and Type.Turret is false,
clearing residual. Preserve the native selector/valid-or-queue8 entry condition.

Migrate serialization, hash, MCV tests and real production callers and remove the
old field/helper/call chain. PerCell reasons share deployment and crush ownership;
only reason2 promotes missions and performs sensor/playfield effects. Complete
the overlay/crush/planning receiver prerequisites before accepting this increment;
Planning Mode has no current Rust owner, so recording its absence is not closure.
Validate native entry/sampler corpora, actual Process behavior, persistence and
callback lifecycle. Final full library/Clippy and fresh independent review remain
required; intermediate focused checks do not certify the increment.

Process-latch implementation now removes the MCV entity field, prelude and
post-movement retry fallback. Drive/Ship class fields carry the latch through
serialization/hash/ownership transfer. The actual ground Process samples only
its non-track branch, clears before reason0 and reloads lifecycle. Native
mission5 in the exact-destination bypass is Guard (not Move). The shared PerCell
receiver distinguishes reasons0/2; only arrival promotes missions and runs
sensor/playfield work. Overlay/complete crush receiver work is still required.

The world TrackProcess entry now owns guard, scalar prefix and paid loop. Removed
`TrackInvocation.fresh_budget` and both collector-side speed calculations.
Geometry/receiver tests pass explicit budgets into the shared paid body and
exclude entry/prefix equivalence. The generic position integrator also no longer
accepts unused Drive/Ship class payloads. This closes a stale caller surface.

Native turn-latch corpus136 rows and entry corpus144 rows regenerated/check
passed independently by root. Turn corpus preserves exact Facing sampler and
original branching; only the completion receiver is an explicit external stub.
New overlay-tail corpus296 rows also checked independently: predicates, retained
entry cell vs live sound XYZ, call ordering, and unconditional +334 binary32
addition/+6B5 clear after damage return0. The whole callback is not represented
by these bounded corpora. Numeric rows include subnormal inputs that current
X87Chop53 rejects; no Rust overlay comparison or completed numeric port claimed.

The planning prerequisite was refined by original constructor/writer/caller
inspection: legacy Foot+520/House+210 paths differ from active PlanMgr+514.
Foot ctor seeds absent slot -1, and the inspected fresh-session graph contains
no legacy append or mode-enable producer. Root reproduced the executable-byte
census. This is a bounded static producer-negative result, not unconditional
unreachability or arbitrary native-save support. Do not implement held-Z as a
substitute for this tail; nonempty legacy state needs its own complete authority
if a supported producer/import path is established. Current missing overlay and
crush receivers remain required independently of this planning distinction.

### Independent review corrections (in progress)

Review rejected the current increment. Required corrections: live Facing must
return before fresh movement; same-cell NavCom and Guard exact-destination
branches must execute their distinct native stop/waypoint receivers and return;
ordinary fresh TurnFirst/refusal must reach the world TrackProcess entry unless
its native retirement/alive guards stop it; both PerCell reasons must evaluate
shield timers using binary_frame. Acceptance includes actual Process with a
nonempty path and no facing_target, and arrival/turn-completion crushing with
offset clocks and both protected and expired victims. The missing synchronous
overlay receiver and full-suite behavioral failures remain independent blockers.

The shared crush receiver now uses the binary-frame clock used by shield
producers. Actual ground Process tests exercise both track families, both
PerCell reasons and both shield kinds with protected/expired victims at offset
clocks. Independent bounded review accepted this correction. Live rotation
now returns before fresh movement; the new queued-path/no-facing_target test
checks the actual caller. Review caught the earlier return bypassing slope
sampling: moved the existing sampler out of movement collection into the sole
ground Process owner before the turn branch, preserving Tube exclusion. The
regression test now covers a changed slope and repeated-frame timer behavior.
This removes a scheduler dependency on reaching movement collection; it does
not complete the still-missing common rest-speed tail or early stop receivers.

Diagnostic15 focused11pass; diagnostic16 full9032pass5fail134ignored, same five
failures. Diagnostic17 stopped at compile due to a private fixture reference;
changed to the existing public test_flat_cell fixture. Diagnostic18 pending.
Root independently re-read original PE spans in both early-return and postfresh
packets. Postfresh returned AL, retirement output, owner alive and retry budget
mode are separate facts; full world fresh migration must preserve all four.

Clock consumer audit also found two production calls to the same crush predicate
in movement_occupancy using sim_tick, while their initial cell query already uses
mcfg.binary_frame. Migrate both inputs and cover expired shielding through actual
ground Process's existing deferred crossing/lifecycle corridor with offset clocks.
This is a caller clock correction, not native parity for that legacy corridor.
Diagnostic18 compiled:9031pass6fail134ignored; new slope fixture had a sparse
vector for a dense indexed grid and sampled no cell. Fix fixture to construct
the complete grid and assert its sampled cell before invoking production.

Diagnostic19 compiled and ran:9033pass5fail134ignored. All eleven turn tests
and the new deferred-crush offset-clock test pass. Independent bounded review
accepted the complete three-caller clock correction and sole slope sampler's
placement before the turn decision. Overall increment still fails acceptance.
Root independently regenerated/check-passed new original rocking1028 and outer
Process continuation254 corpora. Added bounded original-evidence Ghidra plate
notes to4B0500/69FC10/741970, preserved existing notes, saved /gamemd.exe and
read back exact comments. No label/prototype/byte changes. Native-x87 subnormal
prerequisite is now in progress; these results do not validate that later edit.

### Shared finite arithmetic prerequisite (independent candidate)

Clean branch feature/x87-finite-subnormals in ../vera20k-x87-subnormals isolates
finite subnormal load/store support from this unfinished track migration. Initial
candidate library9019pass0fail134ignored; native primitive1780 and overlay296
replays passed; Clippy completed. Fresh critic found House factory-factor math
still duplicated the exact arithmetic because the old shared owner rejected
subnormals. Accepted finding: migrate that caller and scaled_cost to X87Chop53,
retaining only the documented signed max-finite overflow policy adapter. Existing
native House corpus rows preserved; fold10->28 and cost12->36 add original
signed/underflow/overflow comparisons. Fold test now checks recomputation after
membership removal. No save layout changes. Revised final validation/review are
pending; this is not track acceptance.

Critic also identified the independent ruleset damage_spark_spawn_threshold
rational kernel, unbounded tiny-input shift and unproven PC64 assumption. A
read-only native/caller audit is in progress; no finding has been resolved merely
by recording it. Whole-engine acceptance remains open.

PR408 merged as92075219 and integrated into this track branch without altering
94 unrelated WIP files (hash manifest retained in .local). Final x87 candidate:
9019pass0fail134ignored, Clippy1190existingwarnings, four native replays and module
map check PASS; fresh independent re-review PASS after resolving House duplicate.
Track diagnostic19 still has five unresolved failures; no track acceptance claim.


### Signed actual-health cutover in progress

The candidate removes Health.max, stores signed actual HP independently from
estimated reservations, and resolves live Strength at consumers. Snapshot169
rejects old layouts. Raw f64 thresholds replace narrowed/scaled/rational copies.
Cargo04:9080passed/5failed/134ignored17.45s; all26 newly introduced fixture/test
failures resolved, with the five earlier track/replay failures still open.

Independent review disproved removal of Building+6E6: it is saved animation
transition authority. The affected21 instantiated slots must retain selection and
phase across selfheal, missing variants and explicit receiver/repair/allocation/
reload transitions. Full slot ownership consolidation into the existing AnimStore
remains a prerequisite in this same migration gate; restoring a bool alone cannot
certify the prior synthetic looping projection. Selfheal also has a reached
mark-only damage-smoke teardown tail missing from Rust. Correction is underway.

Navigation5F5DD0 and occupied body frame43EF90 now use shared PC53 comparisons.
Reader corpus103inputs/1854executions including618bodyframes independently replayed
and production tests passed. Standalone AudioVisual thresholds now parse without
General, matching independent native ReadAudioVisual. Cargo05 and independent
bounded review passed the parser correction.

Native power corpus429numeric+8admission+16emptyHouse iterations executes original
bytes. The+74 cell-mark gate is independently confirmed. Retained power owners now
recalculate after losing their last qualifying building; totals must clear and
existing blackout time continue. Cargo05 and independent bounded review passed
this integration fix. Other durable evidence: threat672+49, repair/refund147+
14+18, Object718/selfheal131+5fault/constructor32/map416 and conversion272.

Full candidate approval is withheld. Native callback/lifecycle/repair-cost/refund-
owner/survivor gaps, full scanner/Foot/track migrations and final whole-engine audit
remain required. No replay golden rebaseline or new publication has occurred.

Cargo05:9082passed/6failed/134ignored. The sixth failure was an ID collision in
the new selfheal fixture; its correction is staged, not yet rerun. Full animation
ownership migration is underway and is not compile-ready. Original art transition
72 and full body-transition435 corpora passed replay. Native gate, laser-fence
and firestorm body-frame cases require the full receiver guard and retained slot
semantics; ratio-only body projection is insufficient.

### Ordinary fresh track continuation and timer evidence

The ground Process caller omitted TrackProcess after a fresh turn or refusal.
Its native entry must still run and discard residual when the descriptor/queue/
turn gate rejects, without publishing target speed or running the speed prefix.
The production handoff now carries those returns through the existing world
entry after deferred responses and a live Object+90 reload. Earlier live rotation
still returns before fresh selection. The new actual-Process test covers Drive
turn/refusal and Ship turn with positive/negative residuals; no Rust run yet.
Temporary group-admission trace code was removed after retaining its diagnostic.

The composed original outer/entry corpus624 passed generation/replay and fresh
independent replay/static review. Fresh AL does not choose entry; output-byte
retirement and live owner do. Full retirement-byte handling, Ship fresh refusal,
native recursive responses and complete callbacks remain unimplemented. A bounded
static pass is not approval of that wider migration or the current candidate.

Original first-code2 timer/FindPath continuation evidence is now retained in
track_blocked_timers with164 rows/490calls; generation/replay and independent
re-review passed within its declared interior/callback-substitution scope. It exposes the non-Walk compatibility countdown
as an incomplete migration: it discards native frame anchors and ages once per
Process call. Native first-code2 reads anchors and arms only at reached stores.
PathDelay and BlockagePathDelay also have narrowed/clamped Rust representations
that need complete producer/caller migration. No Rust timer behavior has changed.


## Current root checkpoint: signed Foot timing migration (2026-09-20)

HEAD remains bd5928e6 on feature/track-state-authority. No new publication.
Previous goal turn PROGRESS. Ghidra rechecked live PID31056/testProsjekt/
/gamemd.exe: discovery connected and explicit original disassembly HTTP200.

Cargo focused02:12/12 track_turn tests passed. Full06:9083 passed,8 failed,
134 ignored. Unchanged eight-vehicle239-distance regression passes; independent
bounded critique passes. Four failures belong to receiver animation/hash work
and four to older bridge/global/Slice6 replay+dense-position assertions. Global
hash changed05→06; no causal attribution or rebaseline. Receiver's four fixes
are staged but not yet rerun. No Cargo process live at this checkpoint write.

Root has now staged signed/raw [AI] rules delays; removed serialized Simulation
mirrors; migrated production direct/dock/miner/bunker/passenger/unload/rally/sell/
world command/aircraft/spawn/paradrop timing callers. Foot starts always use frame
anchors+i32; obsolete per-Process aging and representation boolean are removed.
Accepted setters share native tail preserving retries; Jumpjet low-level helper
preserves Foot state and outer accepted setter owns timing; paid Walk progress
retains blocked timer. Search reset sites use native core/caller-specific stores
rather than whole FootPathRuntime defaults. See .local/track-blocked-timer-migration.md
for evidence and required unfinished callback/response migration, including code2
post-search rearm, Unit+500, owner retirement, signed retry guards and null setters.
This is not yet a complete/validated timer increment; do not commit or approve it.

Admission's native PathDelay corpus and5rules regression tests frozen; new leaf
production timer regression tests pending. Critic read-only reviewed native reset
and retry differences; re-review current complete timer diff ongoing. Receiver
source frozen for diagnostic Cargo, with full06 fixes, body/slot/art config hash,
192height and420slot scalar comparisons staged. Receiver full operational/power/
storage/constructor coverage remains open; no claim that those staged tests close it.

Next: await testleaf freeze, diagnostic focused rules/timer tests, fix compile and
regression causes, complete native response ownership with affected callers and
fresh independent review. Full final library/bins/Clippy and whole-engine audit,
commit/publish/merge remain required. Do not edit main checkout.


## Terminal validation and current continuation (2026-09-20, after full07)

Root progress this turn changed timer authority/callers and obtained fresh
native criticism plus actual Rust results. Goal remains ACTIVE; no commit,
publication or final candidate approval. HEAD bd5928e6 unchanged.

- .local/foot-timer-focused-01.log session8585 terminal101:6compileerrors,
  all fixed narrowly (air test type path, attached Anim palette context,
  reconstruct RuleSet fixtures rather than adding Clone).
- .local/foot-timer-focused-02.log session55604 terminal0:5/5 native rules tests
  PASS, build6m05s. All11original conversion sites covered within scalar scope.
- .local/foot-timer-production-01.log terminal101:3/4 new production testsPASS:
  direct/regular accepted signed rules and retry preservation, same-frame
  Drive/Ship timer retention using native waiting rows, paidWalk grace retention.
  Snapshot test failed immediatehash: its ScenarioRNG Seed41 differed from
  intentional native load resetSeed0. Root inspected deserialize_scenario_rng_reset
  and existing snapshot tests; admission changed ONLY this fixture's ScenarioRNG
  toSeed0 before save, preserving immediate/continued wholehash assertions.
- .local/signed-health-library-07.log session66894 terminal101:9085PASS,
 8FAIL,134ignored,29.63s. All4receiver-owned06 failures nowPASS. Eightvehicle
 239-distance test stillPASS. Full07 predates the fixture/Hover fixes below.

Full07 failures: newFoot timer snapshot (fixture fix staged); stuckrecovery
(test called every iteration at frame0; rootnow advances0..700, samebound/
assertions); building_anim_slots_roundtrip (receiver diagnosed RNG table seed
mismatch, aligns fixture sessionseed0 with its SimRng0); bridgeRocketeer test
expected wholeFoot default (root replaces with nativeaccepted frame/0,
frame/60,latchfalse,retries256 peroriginalFoot4D96C2..9707). These4fixes are
NOT rerun yet. Earlier4failures remain: bridge/global/Slice6 replay hashes and
denseposition fingerprint. Hashes changed this migration; no blind rebaseline,
no assertion that those are unrelated. Dense fingerprint still17756851045285605503
versus9678228745063827247.

Root additional native-reviewed Hover correction after07: occupancy Clear/
Crushable excludesHover from blockedtimer0, and reached movingHover claim
release clears latchonly per5147DF. Not rerun yet. FullHover search/destination
and callback ordering remain missing; see .local/track-blocked-timer-migration.md
for complete independent findings and original addresses. FullDrive/Ship code2
response/FindPath/retirement/noqueue/samecell/nullsetter gaps remain required.

No root Cargo/native process live after07. Sourcefreeze RELEASED. Receiver is
actively completing full building slot/power/storage/smoke/runtime lifecycle.
Admission authorized narrow art-read receipt/accessor in EXISTING canonical
native_processing registry (not a second owner); receiver consumes inRuleSet/
ArtRegistry. Native registry already owns most FindOrAllocate roots; membership
alone is insufficient for ART read chronology and late allocations. Both must
coordinate noCargo withroot. Critic read-only requirements/diff/results audit
continues; fulltimer migration explicitly UNAPPROVED, not narrowed to tests.

Next safe action: verify agent/source/process state; finish current native
mechanism integration (Hover retaineddestination+MoveTo/search, Foot setters,
DriveShip fresh responses and sharedFindPath) and affected consumers. At next
coherentfreeze, rerun focused4timer tests, stuckrecovery, Rocketeer, animation
snapshot before/full diagnostics as justified. Final lib/bins/Clippy, modulemap,
independent complete review, commits/PR/merge and whole-engine audit remain.

### 2026-09-20 Walk world completion increment

Root moved post-PerCell Walk completion into the existing Simulation callback,
removed the unreachable scalar completion branch/enum and entity-only finalizer,
and retained actual Infantry setter refusal before unconditional speed/head/Stop.
Native original42-row corpus, independent replay, and production comparisonPASS;
world completed-head height/timer/retry/latch testPASS. This is bounded behavior
review, not completion of the whole Walk/Foot migration. Required Stop pending
DoAction callback, post-Mark voice latch and generic finalizer placement ownership
remain. Independent44-row original Foot navigation-coordinate corpus is ready
for shared query migration; no Rust integration claim.

Actual full08:9096PASS/5FAIL/134ignored,37.02s. Four prior fixture failures nowPASS.
Root new coordinate fixture failed I16F16 overflow, then fixed via lossless cell+
subcell decomposition with exact reconstructedXYZ assertion; focused03 terminal0
passes all42cases. Full08 is not retroactively relabeled. The four replay/fingerprint
observed values are unchanged07->08, still causally unresolved. No rebaseline.

Source release after focused03: noCargo/rustc live. Receiver continues common
Building operational/power/storage owner; admission continues coherent pending
Infantry deploy/Doing/sequence ownership with required crush/reload prerequisites.
Full candidate review/lib/bins/Clippy, committed-source modulemap, publication/
merge, and whole-engine final audit remain. Goal active, no publication this turn.

### Foot coordinate integration status

The shared Foot query now feeds input, hut callbacks, Walk completion/path checks,
Drive entity reaim and Track arrival. AtCoord retains its intentional separate
geometry and tolerance. Track arrival now clears its head before querying targets;
post-review source also guards the owner-height query behind physical cell equality
and projects existing Fly/Rocket altitude writers once.

Library diagnostic `.local/foot-coordinate-library-02.log`: 9,104 passed, six
failed, 134 ignored. The new Tube restore fixture lacked immutable-map cache
rehydration (corrected after this run); the power policy fixture omitted deletion
inside a substituted allocation callback (receiver correction pending). Four
replay/fingerprint baselines remain unresolved, with a new global-hash delta that
requires causal attribution. These results do not approve the candidate.

The independently replayed 258-row `building_navigation_coordinate` corpus proves
original query projections within its documented supplied-state scope. Full
Building/Object target dispatch, signed/sparse dock metadata, attached-object
coordinate frames and the native post-TrackProcess reaim boundary are promoted
requirements still being implemented. No publication or whole-engine completion
is claimed. See `.local/foot-coordinate-authority-acceptance.md` for the current
acceptance and original caller evidence.

The signed dock-count prerequisite now preserves BuildingType+1780 as i32 and
centralizes the contact constructor's distinct minimum-one capacity. Aircraft
pad indices are widened through reservations, ammo, mission state and snapshot170.
Original count-reader/contact-gate corpus48 replays successfully; new Rust native
comparison and 300-aircraft restore/release regressions await coordinated Cargo.
Sparse ART offset retention, full Rules-pass array lifecycle, live Radio contact
lookup, attached-object coordinate ownership and Track outer ordering remain
required parts of the same open migration.
