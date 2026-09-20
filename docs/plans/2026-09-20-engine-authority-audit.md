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
   Rust candidate, the full library tests and library Clippy required by ENGINE.md.
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

## Current continuation

Worktree: `C:/Users/enok/Documents/vera20k-engine-authority`.
Branch: `feature/engine-authority-refactor`, based on fetched origin/main.
The main checkout's local-only data is untouched. Increment 1 is implemented and
validated; publication is pending. Independent critic `wallet_critic` gave a scoped
pass after inspecting the full diff, original sources and final logs: no remaining
actionable findings in this wallet increment. Baseline full library
suite passed: 9,015 tests, 134 ignored. An initial candidate compile found a test-only
attempted RuleSet clone; the fixture now reconstructs immutable rules through a shared
parser. Independent criticism found missing active-production restore coverage; the
revised test compares 30 frames of economy and complete factory state with uninterrupted
execution, requires fresh charges, then checks cancellation. Source re-review accepts
the correction. Source commit: `3398ef92`. The dependency map was regenerated from
that committed source (823 modules plus root, 5,240 emitted cargo-modules edges). PR publication and merge
after its checks are the next actions. The full engine goal remains open.

Final local validation for increment 1:

```text
cargo test -p vera20k --lib
test result: ok. 9017 passed; 0 failed; 134 ignored; 0 measured; 0 filtered out; finished in 35.12s
cargo clippy -p vera20k --lib
warning: vera20k (lib) generated 1190 warnings
Finished dev profile [optimized + debuginfo] target(s) in 3m 20s
git diff --check: exit 0
```

Logs remain in this task's worktree `.local/wallet-tests-final.log` and
`.local/wallet-clippy.log`. The two new production tests are
`sim::production::replay_tests::income_spending_and_factory_refund_share_the_runtime_wallet`
and `app::persistence::tests::factory_restore_tests::wallet_survives_prepared_load_and_active_cancellation`.
These establish Rust regression coverage, not native arithmetic parity. The 134
ignored tests were not run; macOS execution is not covered by these Windows-local checks.

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
