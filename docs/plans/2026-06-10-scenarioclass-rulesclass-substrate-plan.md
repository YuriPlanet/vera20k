# ScenarioClass / RulesClass Engine-Substrate Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained.

**Goal:** Close the three behavior-changing parity gaps from the 2026-06-10
ScenarioClass/RulesClass contract: SC-1 (per-match seed wiring), RC-1 (map-INI
rules override), SC-2 (`ScenarioSession` aggregate), in that order.

**Architecture:** All three slices follow "Rust-native structure, gamemd-native
semantics." SC-1/SC-2 introduce `sim/scenario_session.rs` — an app-layer launch
descriptor flowing one-way app→sim at construction (preserves `sim/ ⊥
render/ui/net`), growing into the ScenarioClass-analog session aggregate that
`Simulation` owns. RC-1 extends the existing INI merge chain
(rules.ini → rulesmd.ini → **map overrides**) in the app layer, mirroring
gamemd re-running `Read_INI` on the map file.

**Design Doc:** `docs/contracts/2026-06-10-scenarioclass-rulesclass-engine-substrate-implementation-contract.md`

---

## PARALLEL-SESSION CONSTRAINT (read first)

Four files carry another session's uncommitted radiation work and MUST NOT be
edited by this plan while dirty:
`src/rules/ruleset.rs`, `src/rules/object_type.rs`, `src/sim/game_entity.rs`,
`src/sim/world/world_spawn.rs`.

- SC-1 and RC-1 are designed to not touch them at all.
- SC-2's field move MUST touch `world_spawn.rs:831` (`self.game_options.…`).
  **Gate:** before starting Task 12, run `git status --short` — if any of the
  four files is still modified, STOP at the end of RC-1 and report; do not
  start SC-2.
- `ruleset.rs` line numbers in the contract are stale by the parallel diff
  (+97 lines; `RuleSet::from_ini` is currently at `ruleset.rs:1538`). Reference
  that file by symbol name only.

## Grounding Summary

- **Binary truth comes from the contract's sources** —
  [RANDOM_SCENARIO_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RANDOM_SCENARIO_ENGINE_SUBSTRATE_SERVICE_STUDY.md) (seed pipeline
  `Init_Random_Number_System @ 0x0052FC20`, ScenarioClass field map, lifecycle)
  and [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) (`Process @ 0x006686C0` re-running
  `Read_INI @ 0x00668BF0` on the map file). Every load-bearing claim was
  re-verified live in those sessions; no new Ghidra work is required for these
  three slices (confidence: verified-from-binary, by citation).
- **Rust anchors re-verified this session:** `DEFAULT_SIM_SEED`
  (`world/mod.rs:85`), `with_seed` dual seeding + `debug_assert_eq!`
  (`mod.rs:520-577`), `reseed_both` (`mod.rs:627-634`), three streams hashed in
  fixed order (`world_hash.rs:66-77`), `SNAPSHOT_VERSION = 20`
  (`snapshot.rs:41`), replay header recorded from `sim.seed` but playback never
  applies `header.seed` (`app_sim_tick.rs:259-266`, `replay.rs:76-91`),
  `load_rules_ini` two-file merge (`app_init_helpers.rs:247-289`), map INI
  retained on `MapFile.ini` (`map_file.rs:198`), `IniFile::merge`
  (`ini_parser.rs:304-324`), lazy fog bounds (`vision/mod.rs:464-468`,
  `resolve_bounds` at `:535`), MP start waypoints 0..=7
  (`map/waypoints.rs:19,54`), golden baseline `GLOBAL_HARNESS_FINAL_HASH`
  (`global_parity_harness_tests.rs:44`), `SimRng::new` truncates the u64 seed
  to u32 (`rng.rs:25-34`).
- **Repo patterns mirrored:** ObjectSubstrate relocation (snapshot bump 15/16
  precedent for field moves + version bump), `GameOptions`
  defaults-then-override parse (`game_options.rs`), intent-named RNG accessors
  + routing tests, golden-harness re-baseline ceremony (documented one-line
  reason on the constant).
- **INI keys:** `[MultiplayerDialogSettings]` (already parsed),
  `[General] BuildSpeed=`, `[CombatDamage] C4Delay=` (already parsed —
  `RuleSet.c4_delay_ticks`), map `[Header]` Size/LocalSize, map `[Waypoints]`,
  map `[Basic] Name=`.
- **Still unknown** (stays out of scope, per contract): whether a map INI may
  *allocate* new type records (RC-1 BLOCKED sub-question), LANGRULE.INI content
  (RC-6), cell-action timer (SC-7), `g_MapGenRng` seeding on `.SED` maps (SC-6).

## Key Technical Decisions

- **Descriptor seed is `u32`**: gamemd's negotiated `g_RngSeed` is u32 and
  `SimRng::new` already truncates (`rng.rs:32`); the descriptor carries `u32`
  and widens at the `Simulation` edge. `Simulation.seed` stays `u64` so the
  bincode layout and `ReplayHeader` JSON are untouched in SC-1. Resolves the
  contract's YELLOW seed-truncation question. — **Confidence:** high.
  **Source:** study §5 C-SEED via contract; `rng.rs:25-34`.
- **`Simulation::new()` survives as a test/dev fallback** (250+ test call
  sites; `tests/` integration tests compile without `cfg(test)`, so gating is
  not viable). AT-3 is enforced by a source-scan tripwire test over the launch
  path files instead. — **Confidence:** high. **Source:** caller grep this
  session.
- **SP entropy lives in the app layer** (`SystemTime`-based, logged at
  launch). The contract explicitly does not bind us to gamemd's entropy
  source, only to one shared seed fixed before any setup-phase draw.
  — **Confidence:** high. **Source:** contract SC-1 "Required Rust Changes" #1.
- **RC-1 merges by value-override only**: a map section merges **only if the
  section already exists** in the merged rules INI, and numbered-list registry
  sections are skipped entirely (explicit exclusion list + all-numeric-keys
  guard). Allocation-from-map is the contract's BLOCKED question — skipping
  registries avoids both implementing allocation and the catastrophic wrong
  semantics of index-keyed overwrite (`0=` in a map [VehicleTypes] replacing
  rules entry 0). — **Confidence:** high for "don't allocate" (contract);
  medium for the exact exclusion list (flag for `/review-plan`). **Source:**
  contract RC-1; [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) §9.3.
- **SC-2 moves fields with compiler-driven renames, not sed**: delete the five
  moved fields, fix every `E0609` the compiler reports (~365 references across
  `tick`/clock/`game_options`/`seed`). Hash folds keep their exact current
  order so the move lands hash-neutral (AT-7). — **Confidence:** high.
  **Source:** ref-count grep this session; ObjectSubstrate precedent
  (snapshot.rs:18-21 comments).
- **One `SNAPSHOT_VERSION` bump (20→21) for all of SC-2**: the full
  `ScenarioSession` struct (moved + new fields) is serialized from the first
  move task, so the bincode layout changes once; the later hash fold is
  documented on the same version comment. — **Confidence:** medium (flag for
  `/review-plan`). **Source:** snapshot.rs bump-history comments.
- **`rules_hash` stays registry-only** (`app_sim_tick.rs:1356-1363` hashes
  only the four ID lists). RC-1 value overrides therefore do NOT change
  `rules_hash` — the contract's risk note assumed a value-sensitive hash that
  doesn't exist. Making `rules_hash` value-sensitive is a pre-existing gap,
  deferred (not in contract scope). — **Confidence:** high. **Source:** read
  this session.

## Open Questions

### Resolved During Planning

- *u64 vs u32 seed* — descriptor u32, `Simulation.seed` stays u64 (above).
- *Can AT-3 be a compile-time gate?* — No: `tests/determinism_replay.rs` (an
  integration test, built without `cfg(test)`) constructs `Simulation`
  directly, and 250+ unit tests call `new()`. Tripwire source-scan test
  instead.
- *Where can RC-1 get the map INI?* — `MapFile.ini` (`map_file.rs:198`) is in
  scope at `app_init.rs:418` before `load_rules_ini` is called; no re-read
  needed. The startup-shell call (`app.rs:2376`) has no map — passes `None`.
- *Does the replay record path already carry the seed?* — Yes
  (`app_sim_tick.rs:263`); only playback application is missing.

### Deferred to Implementation

- Exact tick values asserted in AT-9's C4Delay check depend on the
  minutes→ticks conversion constant in `ruleset.rs` (read it at execution; do
  not edit that file).
- Whether `production_replay_tests.rs` fixture headers carry seeds mismatching
  their sims (would trip the new `debug_assert`) — check and align in Task 4.
- The post-SC-2 golden baseline value — captured from the first green run,
  documented on the constant.

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Create | `src/sim/scenario_session.rs` | `ScenarioDescriptor` (app→sim launch data) + `ScenarioSession` aggregate (SC-2) |
| Create | `tests/launch_seed_guard.rs` | AT-3 tripwire: launch path must not use `Simulation::new()` |
| Modify | `src/sim/mod.rs` | declare `pub mod scenario_session` |
| Modify | `src/sim/world/mod.rs` | `from_descriptor`, DEFAULT_SIM_SEED demotion; SC-2 field moves |
| Modify | `src/sim/world/world_hash.rs` | SC-2: session-routed folds (same order), then new-field folds |
| Modify | `src/sim/replay.rs` | seed-fidelity guard in `ReplayRunner::run` |
| Modify | `src/sim/snapshot.rs` | SC-2: version 20→21; `sim.tick` → session path |
| Modify | `src/sim/vision/mod.rs` | SC-2: construction-time bounds beat lazy `resolve_bounds` |
| Modify | `src/app_init_helpers.rs` | entropy helper; `spawn_entities` descriptor param; `load_rules_ini` map param; AT-9/AT-10 tests |
| Modify | `src/app_init.rs` | build descriptor at launch; pass map INI into rules load |
| Modify | `src/app.rs` | startup `load_rules_ini` caller gains `None` arg |
| Modify | `src/app_sim_tick.rs` | SC-2: replay header from session |
| Modify | `src/app_skirmish.rs` | SC-2: fill `start_slot_houses` |
| Modify | `src/rules/ini_parser.rs` (+`ini_parser_tests.rs`) | `merge_rules_overrides` |
| Modify | `src/sim/world/global_parity_harness_tests.rs` | AT-8 stream pins; SC-2 baseline ceremony |
| Modify | `tests/determinism_replay.rs` | AT-2 |

**Never modified by this plan while dirty:** `src/rules/ruleset.rs`,
`src/rules/object_type.rs`, `src/sim/game_entity.rs`,
`src/sim/world/world_spawn.rs` (SC-2 gate above).

## Interface Changes

- `spawn_entities(…)` gains `descriptor: &ScenarioDescriptor` (sole caller:
  `app_init.rs:573`).
- `load_rules_ini(asset_manager)` →
  `load_rules_ini(asset_manager, map_rules: Option<&IniFile>)` (callers:
  `app_init.rs:418`, `app.rs:2376`).
- `IniFile` gains `merge_rules_overrides(&mut self, patch: &IniFile) -> usize`.
- SC-2: `Simulation` loses pub fields `tick`, `total_sim_ms`, `binary_frame`,
  `game_options` (and pub(crate) `seed`); gains `pub session: ScenarioSession`.
  Every consumer re-routes (compiler-enforced).

## Sim Checklist

- [x] No float in sim logic (descriptor/session: integers, Strings, BTreeMap).
- [x] New SC-2 state serialized + hashed; `SNAPSHOT_VERSION` 20→21.
- [x] No render/ui/sidebar/audio/net deps: descriptor is plain data built in
      the app layer; entropy (`SystemTime`) stays app-side.
- [x] Tick ordering untouched; clock commit point (`mod.rs:1948`) keeps its
      position, only its storage moves.
- [x] BTreeMap for waypoint/slot tables (deterministic iteration + hashing).

## Risk Areas

- **Seed entropy breaks accidental determinism assumptions** — any non-test
  flow that implicitly relied on `DEFAULT_SIM_SEED` now varies per run. The
  golden harness and all tests pin seeds explicitly; replay records the seed.
  Regression net: full test suite + AT-1/AT-2.
- **`ReplayRunner::run` debug_assert** can trip existing fixtures whose header
  seed ≠ sim seed (`production_replay_tests.rs`) — Task 4 aligns them.
- **RC-1 over-merge** would corrupt type registries — the exclusion list +
  numeric-key guard + "existing sections only" rule are each tested in
  Task 8.
- **SC-2 mechanical move** is wide (~365 refs incl. `sim/` internals); landed
  as a single hash-neutral commit verified by the unchanged golden baseline
  before any new field is hashed (AT-7).

## Parity-Critical Items

| Task # | Item | Why it matters | Verification |
|--------|------|----------------|--------------|
| 1–3 | One u32 seed fixed before ANY setup-phase draw (incl. random country/color resolution at `app_skirmish.rs:178`) | gamemd fixes `g_RngSeed` before `Start_Scenario`; a draw before seeding desyncs MP from tick −1 | AT-1 (sibling sims), AT-2 (replay), existing `rng_routing_tests` |
| 1 | Seed → both streams byte-identically, mapgen stays zero-state | Study C-SEED/C-STREAMS; divergence-by-consumption only | `from_descriptor` vs `with_seed` equivalence test; `debug_assert_eq!` at `mod.rs:575` |
| 4 | Replay playback constructed from `header.seed` | Replays silently keyed to a constant break the moment SC-1 lands | AT-2 corrupt-header divergence |
| 7 | Per-stream cursor pins at checkpoint ticks | Catches silent cross-stream misrouting that total-hash equality can mask | AT-8 in golden harness |
| 8–10 | Map rules-override: last-definition-wins, value-only, fires on every map shipping overrides, all match long | gamemd `Process` re-runs `Read_INI` on the map; today such maps play with stock values | AT-9 fixture (BuildSpeed + C4Delay) |
| 8 | Registry sections NOT merged from maps | Index-keyed overwrite would replace e.g. rules vehicle 0 — observable instantly; allocation semantics unverified (BLOCKED) | Task 8 unit tests |
| 13 | Field move hash-neutral (fold order unchanged) | Hash contract: any reorder silently re-baselines the desync detector | AT-7: `GLOBAL_HARNESS_FINAL_HASH` unchanged (669004916847079430) |
| 14 | Bounds known before first tick (no zero-dim fog window) | gamemd map bounds are authoritative at load; first-tick vision must not run against 0×0 | AT-5 |
| 14–15 | Waypoint table + slot→house sim-resident and hashed | Setup-phase deficit fill draws from the Scen stream — waypoint state is lockstep state | AT-6 |

---

## Tasks

### Task 1: `ScenarioDescriptor` + `Simulation::from_descriptor`

**Why:** The seed-injection interface everything else consumes; defining it
first keeps SC-1/SC-2 on one path.

**Files:**
- Create: `src/sim/scenario_session.rs`
- Modify: `src/sim/mod.rs` (module decl), `src/sim/world/mod.rs`

**Pattern:** module-with-`//!`-header + `#[cfg(test)] mod tests` (mirrors
`src/sim/game_options.rs`).

**Step 1: Create the module**

```rust
// src/sim/scenario_session.rs
//! Scenario session substrate — the launch descriptor the app layer feeds the
//! sim exactly once at construction.
//!
//! Mirrors the original engine fixing one per-match RNG seed before any
//! setup-phase draw, then seeding the scenario and main streams identically.
//! Data flows one-way app→sim; this module depends only on sim/ siblings.

/// Everything the app layer decides about a session before the sim exists.
/// Built from the lobby/launch flow — never hardcoded inside sim/.
#[derive(Debug, Clone, Default)]
pub struct ScenarioDescriptor {
    /// The negotiated per-match seed. 32 bits wide because the original's
    /// negotiated seed is 32 bits and the RNG seeder consumes exactly 32; SP
    /// entropy, future MP handshake, and replay headers all funnel through
    /// this one field.
    pub seed: u32,
}

impl ScenarioDescriptor {
    /// Reconstruct the descriptor a recorded match was created from, so
    /// playback seeds the sim exactly as the original run did.
    pub fn from_replay_header(header: &crate::sim::replay::ReplayHeader) -> Self {
        Self { seed: header.seed as u32 }
    }
}
```

**Step 2: Declare the module** in `src/sim/mod.rs` (alphabetical with its
siblings): `pub mod scenario_session;`

**Step 3: Add the constructor** in `src/sim/world/mod.rs`, next to
`with_seed` (after `mod.rs:577`):

```rust
/// Construct a session simulation from an app-layer launch descriptor.
/// The only entry real launches use; `new()`/`with_seed()` remain for
/// tests and dev tooling.
pub fn from_descriptor(desc: &crate::sim::scenario_session::ScenarioDescriptor) -> Self {
    Self::with_seed(u64::from(desc.seed))
}
```

**Step 4: Demote the default seed.** Replace the doc comment at
`world/mod.rs:84-85` with:

```rust
/// Dev/test fallback seed. Real launches negotiate a per-match seed through
/// `ScenarioDescriptor`; nothing on the launch path may rely on this value.
const DEFAULT_SIM_SEED: u64 = 0x5EED_CAFE_D15E_A5E5;
```

**Step 5: Add tests** at the bottom of `scenario_session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::world::Simulation;

    #[test]
    fn from_descriptor_equals_with_seed_widened() {
        let a = Simulation::from_descriptor(&ScenarioDescriptor { seed: 0xDEAD_BEEF });
        let b = Simulation::with_seed(0xDEAD_BEEF);
        assert_eq!(a.state_hash(), b.state_hash());
    }

    #[test]
    fn from_replay_header_roundtrips_u32_seed() {
        let header = crate::sim::replay::ReplayHeader {
            version: 1, tick_hz: 15, seed: 0x1234_5678,
            map_name: String::new(), rules_hash: 0,
        };
        assert_eq!(ScenarioDescriptor::from_replay_header(&header).seed, 0x1234_5678);
    }
}
```

**Step 6: Verify** — `cargo test -p vera20k scenario_session` → PASS; read the
literal `test result:` line.

**Step 7: Commit** — `sim: SC-1 T1 — ScenarioDescriptor + Simulation::from_descriptor (u32 negotiated seed; DEFAULT_SIM_SEED demoted to dev/test fallback)`

### Task 2: App-layer entropy + descriptor threading

**Why:** Wires a real per-match seed into every launch (gamemd fixes the seed
before `Start_Scenario`); without this SC-1 changes nothing observable.

**Files:**
- Modify: `src/app_init_helpers.rs` (helper + `spawn_entities`),
  `src/app_init.rs` (build + pass descriptor)

**Pattern:** app-layer helpers in `app_init_helpers.rs` (mirrors
`load_skirmish_game_options`).

**Step 1: Entropy helper** in `app_init_helpers.rs` (near the top-level fns):

```rust
/// Draw one fresh per-match seed. The SP analog of the original fixing its
/// global RNG seed once per game before any setup-phase draw; we are bound to
/// reaching one shared u32 seed, not to the original's entropy source. MP
/// will hand the host's seed over the wire through the same descriptor.
pub(crate) fn generate_match_seed() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.subsec_nanos() ^ (now.as_secs() as u32).rotate_left(16)
}
```

**Step 2: `spawn_entities` gains the descriptor.** Add parameter
`descriptor: &crate::sim::scenario_session::ScenarioDescriptor` to the
signature (`app_init_helpers.rs:388-404`) and replace line 410:

```rust
let mut sim: Simulation = Simulation::from_descriptor(descriptor);
```

**Step 3: Build + pass it** in `app_init.rs`. Immediately before the
`spawn_entities` call (`app_init.rs:573`):

```rust
let scenario_descriptor = crate::sim::scenario_session::ScenarioDescriptor {
    seed: crate::app_init_helpers::generate_match_seed(),
};
log::info!("Match seed: 0x{:08X}", scenario_descriptor.seed);
```

and add `&scenario_descriptor` as the new argument. The seed log line is the
repro handle for bug reports until replay save/load UI exists.

**Step 4: Verify** — `cargo check -p vera20k` clean; launch path compiles with
exactly one `spawn_entities` caller updated (`rg -n "spawn_entities\(" src/`
must show only the def and `app_init.rs`).

**Step 5: Commit** — `app: SC-1 T2 — per-match entropy seed wired through ScenarioDescriptor into sim construction (logged for repro)`

### Task 3: AT-1 — sibling-sim seed-sync test

**Why:** Proves seed injection reaches all three streams before tick 0 and
that descriptor-equal sims stay in lockstep; the MP-correctness core of SC-1.

**Files:**
- Modify: `src/sim/scenario_session.rs` (tests mod)

**Step 1: Add the test** (same scripted-commands shape as
`global_parity_harness_tests.rs`; minimal world is fine — determinism is the
assertion, not scenario richness):

```rust
#[test]
fn mp_sibling_rng_state_matches_after_seed_sync() {
    use crate::map::entities::{EntityCategory, MapEntity};
    use crate::sim::command::{Command, CommandEnvelope};
    use std::collections::BTreeMap;

    fn build(seed: u32) -> Simulation {
        let mut sim = Simulation::from_descriptor(&ScenarioDescriptor { seed });
        let entity = MapEntity {
            owner: "Americans".to_string(), type_id: "MTNK".to_string(),
            health: 256, cell_x: 2, cell_y: 2, facing: 64,
            category: EntityCategory::Unit, sub_cell: 0, veterancy: 0, high: false,
        };
        sim.spawn_from_map(&[entity], None, &BTreeMap::new());
        sim
    }
    fn run_300(sim: &mut Simulation) -> Vec<u64> {
        let heights = BTreeMap::new();
        let owner = sim.interner.get("Americans").expect("interned");
        (0..300u64)
            .map(|t| {
                let cmds = if t == 5 {
                    vec![CommandEnvelope::new(owner, 6, Command::Move {
                        entity_id: 1, target_rx: 20, target_ry: 2,
                        queue: false, group_id: None,
                    })]
                } else {
                    Vec::new()
                };
                sim.advance_tick(&cmds, None, &heights, None, None, 67).state_hash
            })
            .collect()
    }

    let (mut a, mut b) = (build(0xA5EED), build(0xA5EED));
    assert_eq!(run_300(&mut a), run_300(&mut b), "same descriptor seed => lockstep");
    assert_eq!(a.scenario_rng.state(), b.scenario_rng.state());
    assert_eq!(a.main_rng.state(), b.main_rng.state());
    assert_eq!(a.mapgen_rng.state(), b.mapgen_rng.state());

    let mut c = build(0xA5EED + 1);
    assert_ne!(a.state_hash(), run_300(&mut c).last().copied().map(|_| c.state_hash()).unwrap(),
        "different seeds must diverge");
}
```

(`scenario_rng`/`main_rng`/`mapgen_rng` are `pub(crate)`; this test lives
inside `sim/`, so direct access works. If field privacy bites, route through
`reseed_both`-style test accessors instead — do not widen visibility.)

**Step 2: Verify** — `cargo test -p vera20k mp_sibling_rng_state` → PASS.

**Step 3: Commit** — `test(sim): SC-1 T3 — AT-1 sibling sims from one descriptor seed stay in per-stream lockstep; different seeds diverge`

### Task 4: Replay playback constructs from `header.seed` (AT-2)

**Why:** Replays currently record the seed but never apply it — playback
determinism silently rests on the old constant, which Task 2 just removed.

**Files:**
- Modify: `src/sim/replay.rs`, `tests/determinism_replay.rs`
- Check/align: `src/sim/production/production_replay_tests.rs` fixture headers

**Step 1: Seed-fidelity guard** at the top of `ReplayRunner::run`
(`replay.rs:76-84`):

```rust
// Playback must be constructed from the recorded seed (the descriptor path:
// `ScenarioDescriptor::from_replay_header`). A sim seeded differently than
// the header it replays is a guaranteed silent divergence.
debug_assert_eq!(
    sim.seed, replay.header.seed,
    "replay playback sim must be constructed from header.seed"
);
```

**Step 2: Align existing fixtures.** Run
`cargo test -p vera20k replay 2>&1 | tail -40`. Any fixture that records a
header seed different from its sim's construction seed now panics in debug —
fix the fixture (set the header `seed:` to the sim's seed, or construct the
sim `with_seed(header_seed)`), never weaken the assert. Known suspects:
`production_replay_tests.rs:53` (sim via `new()`) vs `:163` (header literal);
`determinism_replay.rs` and `global_parity_harness_tests.rs` already agree.

**Step 3: AT-2 test** in `tests/determinism_replay.rs`:

```rust
#[test]
fn replay_reapplies_header_seed() {
    use vera20k::sim::scenario_session::ScenarioDescriptor;

    // Record under a descriptor seed.
    let desc = ScenarioDescriptor { seed: 0x00A1_1CE5, ..Default::default() };
    let mut sim = Simulation::from_descriptor(&desc);
    // (reuse make_test_sim's entity block, but seeded via the descriptor)
    /* spawn the MTNK exactly as make_test_sim does */
    let mut replay = ReplayLog::new(ReplayHeader {
        version: 1, tick_hz: 30, seed: sim.seed,
        map_name: "seed_roundtrip".to_string(), rules_hash: 0,
    });
    let grid = PathGrid::new(32, 32);
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let mut live: Vec<u64> = Vec::new();
    for _ in 0..40 {
        let due = vec![]; // movement-free is fine; RNG/clock still advance the hash
        let r = sim.advance_tick(&due, None, &heights, Some(&grid), None, TICK_MS);
        replay.record_tick(r.tick, due, r.state_hash);
        live.push(r.state_hash);
    }

    // Playback constructed FROM the header — the new contract.
    let mut playback = Simulation::from_descriptor(
        &ScenarioDescriptor::from_replay_header(&replay.header));
    /* spawn the same MTNK */
    let replayed = ReplayRunner::run(&mut playback, &replay, None, &heights, Some(&grid), TICK_MS);
    assert_eq!(live, replayed, "playback from header.seed must match the recorded timeline");

    // Corrupt the header: a consistent-but-wrong seed must diverge.
    let mut corrupted = replay.clone();
    corrupted.header.seed ^= 1;
    let mut wrong = Simulation::from_descriptor(
        &ScenarioDescriptor::from_replay_header(&corrupted.header));
    /* spawn the same MTNK */
    let diverged = ReplayRunner::run(&mut wrong, &corrupted, None, &heights, Some(&grid), TICK_MS);
    assert_ne!(live, diverged, "a corrupted header seed must not reproduce the timeline");
}
```

(Replace the `/* spawn */` comments with the literal `MapEntity` block from
`make_test_sim` — copy it; tasks are self-contained, the helper hardcodes
`with_seed`.)

**Step 4: Verify** — `cargo test -p vera20k --test determinism_replay` and
`cargo test -p vera20k replay` → all PASS (read the literal result lines).

**Step 5: Commit** — `sim: SC-1 T4 — AT-2 replay playback constructs from header.seed; ReplayRunner guards sim/header seed fidelity`

### Task 5: AT-3 — launch-path seed-guard tripwire

**Why:** Keeps future edits from quietly reintroducing the constant-seed
launch; compile-gating is impossible (integration tests + 250 unit-test
callers), so a source-scan test is the enforceable form.

**Files:**
- Create: `tests/launch_seed_guard.rs`

**Step 1: Write the guard**

```rust
//! AT-3 guard: real launches must construct the sim through
//! `ScenarioDescriptor` — never `Simulation::new()`/`Simulation::default()`.
//! Source-scan tripwire because `new()` must stay available to the 250+
//! test fixtures (integration tests compile without cfg(test)).

const LAUNCH_PATH_SOURCES: &[&str] = &[
    "src/app_init_helpers.rs",
    "src/app_init.rs",
    "src/app_loading.rs",
];

#[test]
fn launch_path_never_uses_the_default_sim_seed() {
    for rel in LAUNCH_PATH_SOURCES {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {rel}: {e}"));
        // Strip the in-file test module: everything from the first
        // `#[cfg(test)]` to EOF is fixture territory.
        let live = src.split("#[cfg(test)]").next().unwrap_or(&src);
        for needle in ["Simulation::new()", "Simulation::default()"] {
            assert!(
                !live.contains(needle),
                "{rel} uses {needle} on the launch path — construct via \
                 ScenarioDescriptor/Simulation::from_descriptor instead"
            );
        }
    }
}
```

(If `src/app_loading.rs` doesn't exist, drop it from the list — verify with
`ls src/app_loading.rs` first.)

**Step 2: Verify** — `cargo test -p vera20k --test launch_seed_guard` → PASS.

**Step 3: Commit** — `test: SC-1 T5 — AT-3 tripwire pins the launch path to descriptor-constructed sims`

### Task 6: AT-8 — per-stream cursor pins in the golden harness

**Why:** Total-hash equality can mask a draw routed to the wrong stream if a
compensating error exists; per-stream checkpoints catch misrouting directly.

**Files:**
- Modify: `src/sim/world/global_parity_harness_tests.rs`

**Step 1:** In `global_skirmish_replay_is_deterministic_and_baseline_stable`,
capture per-stream fingerprints during the record pass and assert equality in
the replay pass at checkpoint ticks:

```rust
const STREAM_CHECKPOINT_TICKS: &[u64] = &[150, 300, 450, 599];
// record pass, inside the tick loop after advance_tick:
if STREAM_CHECKPOINT_TICKS.contains(&tick) {
    recorded_streams.push((tick, rec.scenario_rng.state(), rec.main_rng.state(), rec.mapgen_rng.state()));
}
```

Replay pass: replace the single `ReplayRunner::run` call with a manual loop
over `log.ticks` (same `advance_tick` arguments — this is what `run` does),
collecting hashes AND stream fingerprints at the same ticks; assert the
existing tick-by-tick hash equality unchanged, plus:

```rust
assert_eq!(recorded_streams, replayed_streams,
    "per-stream cursor pin: a draw moved streams between record and replay");
// mapgen must stay the zero-state fingerprint on a fixed map (never seeded).
let zeroed = crate::sim::rng::SimRng::zeroed().state();
for (_, _, _, mapgen) in &recorded_streams {
    assert_eq!(*mapgen, zeroed, "mapgen stream consumed/seeded on a fixed map");
}
```

Wait — the bridge-repair walker variant legitimately draws from `mapgen_rng`
(zero-state draws return 0 but advance no state words; `state()` is unchanged
by zero-draws only if index words also stay put — verify by reading
`SimRng::next_u32`: if the indices advance, drop the `zeroed` assert and keep
only record-vs-replay equality). Decide from the code, not assumption.

**Step 2: Verify** — `cargo test -p vera20k global_skirmish_replay` → PASS,
`GLOBAL_HARNESS_FINAL_HASH` untouched.

**Step 3: Commit** — `test(sim): SC-1 T6 — AT-8 per-stream cursor pins at golden-harness checkpoints`

### Task 7: SC-1 closeout — full verification

**Why:** SC-1 is behavior-changing on the launch path; prove nothing else moved.

**Step 1:** `cargo test -p vera20k` (full) — read the literal `test result:`
lines; every suite green.
**Step 2:** `cargo clippy -p vera20k` — no new warnings in touched files.
**Step 3:** If any failure traces to the four parallel-session files, do NOT
fix them — re-run scoped to your own suites and note it.
**Step 4: Commit** anything outstanding —
`sim: SC-1 complete — negotiated per-match seed wired end-to-end (entropy launch, replay header authority, guards)`

### Task 8: RC-1 — `IniFile::merge_rules_overrides`

**Why:** The merge primitive for map rules overrides; isolated and unit-tested
before any caller exists.

**Files:**
- Modify: `src/rules/ini_parser.rs`, `src/rules/ini_parser_tests.rs`

**Pattern:** sits beside `IniFile::merge` (`ini_parser.rs:304-324`), same
section/key access idioms.

**Step 1: The method + exclusion list** (in the same `impl IniFile` block):

```rust
/// Numbered-list registry sections a map override pass must not touch.
/// Registries use find-or-allocate-by-name semantics in the original; merging
/// them by numeric key would *replace* unrelated entries, and whether a map
/// may allocate new type records at all is unverified. Value sections only.
const MAP_OVERRIDE_EXCLUDED_SECTIONS: &[&str] = &[
    "InfantryTypes", "VehicleTypes", "AircraftTypes", "BuildingTypes",
    "TerrainTypes", "SmudgeTypes", "OverlayTypes", "Tiberiums",
    "SuperWeaponTypes", "Countries", "Animations", "VoxelAnims",
    "Warheads", "Projectiles", "Sides",
];

/// Merge a map file's rules-shaped overrides over this (already
/// rules+rulesmd-merged) INI: last definition wins, but only for sections
/// that already exist here, and never for type-registry lists. Mirrors the
/// original re-reading its rules from the map file after the main load,
/// minus the (unverified) ability to allocate new type records.
/// Returns the number of keys applied (0 = the map ships no overrides).
pub fn merge_rules_overrides(&mut self, patch: &IniFile) -> usize {
    let mut applied = 0;
    for patch_key in &patch.section_order {
        let Some(patch_section) = patch.sections.get(patch_key) else { continue };
        if MAP_OVERRIDE_EXCLUDED_SECTIONS
            .iter()
            .any(|s| s.eq_ignore_ascii_case(patch_key))
        {
            continue;
        }
        let Some(base_section) = self.sections.get_mut(patch_key) else {
            // Value-override only: a section the rules never declared is map
            // data (or would be an allocation) — skip silently.
            continue;
        };
        // Belt-and-braces: an all-numeric-key section is a registry list even
        // if it's not on the exclusion list yet.
        let mut keys = patch_section.keys().peekable();
        let all_numeric = keys.peek().is_some()
            && patch_section.keys().all(|k| k.parse::<u32>().is_ok());
        if all_numeric {
            log::warn!(
                "map [{patch_key}] is a numbered list — registry overrides from \
                 maps are not supported; section skipped"
            );
            continue;
        }
        for key in patch_section.keys() {
            if let Some(val) = patch_section.get(key) {
                base_section.set(key, val);
                applied += 1;
            }
        }
    }
    applied
}
```

(Adjust field/method names to the actual `IniFile`/`IniSection` API —
`section_order`, `sections`, `keys()`, `get()`, `set()` all exist per
`merge()` at `ini_parser.rs:304-324`. Match `merge`'s exact key-matching
semantics; do not invent case-folding `merge` doesn't have.)

**Step 2: Unit tests** in `ini_parser_tests.rs`:

```rust
#[test]
fn map_overrides_merge_only_existing_value_sections() {
    let mut rules = IniFile::from_str(
        "[General]\nBuildSpeed=.7\nFlightLevel=1500\n[CombatDamage]\nC4Delay=.03\n");
    let map = IniFile::from_str(
        "[General]\nBuildSpeed=2\n[CombatDamage]\nC4Delay=.06\n\
         [Basic]\nName=TestMap\n[Waypoints]\n0=45035\n");
    let applied = rules.merge_rules_overrides(&map);
    assert_eq!(applied, 2);
    assert_eq!(rules.section("General").unwrap().get("BuildSpeed"), Some("2"));
    assert_eq!(rules.section("General").unwrap().get("FlightLevel"), Some("1500"));
    assert_eq!(rules.section("CombatDamage").unwrap().get("C4Delay"), Some(".06"));
    assert!(rules.section("Basic").is_none(), "map-only sections must not allocate");
}

#[test]
fn map_overrides_skip_type_registries_and_numbered_lists() {
    let mut rules = IniFile::from_str("[VehicleTypes]\n0=MTNK\n[Animations]\n0=RING1\n");
    let map = IniFile::from_str("[VehicleTypes]\n0=EVILTANK\n[Animations]\n0=EVILANIM\n");
    let applied = rules.merge_rules_overrides(&map);
    assert_eq!(applied, 0);
    assert_eq!(rules.section("VehicleTypes").unwrap().get("0"), Some("MTNK"));
}

#[test]
fn map_overrides_no_op_without_rules_shaped_sections() {
    let mut rules = IniFile::from_str("[General]\nBuildSpeed=.7\n");
    let map = IniFile::from_str("[Basic]\nName=Clean\n[IsoMapPack5]\n1=AAAA\n");
    assert_eq!(rules.merge_rules_overrides(&map), 0);
    assert_eq!(rules.section("General").unwrap().get("BuildSpeed"), Some(".7"));
}
```

**Step 3: Verify** — `cargo test -p vera20k ini_parser` → PASS.

**Step 4: Commit** — `rules: RC-1 T8 — IniFile::merge_rules_overrides (map value overrides, existing sections only, registries excluded)`

### Task 9: RC-1 — wire map overrides into `load_rules_ini`

**Why:** The behavior flip: maps shipping `[General]`/`[CombatDamage]`/…
overrides stop playing with stock values.

**Files:**
- Modify: `src/app_init_helpers.rs:247-289`, `src/app_init.rs:418`,
  `src/app.rs:2376`

**Step 1:** Grow the signature and add the third merge step after the
rulesmd merge (`app_init_helpers.rs:277`):

```rust
pub(crate) fn load_rules_ini(
    asset_manager: &AssetManager,
    map_rules_overrides: Option<&IniFile>,
) -> Option<RuleSet> {
    // … existing steps 1–2 unchanged …

    // Step 3: map rules overrides — the original re-reads its rules from the
    // map file after the main load, so maps may override value sections.
    if let Some(map_ini) = map_rules_overrides {
        let applied = ini.merge_rules_overrides(map_ini);
        if applied > 0 {
            log::info!("Applied {applied} map rules-override key(s)");
        }
    }

    // … RuleSet::from_ini(&ini) unchanged …
}
```

**Step 2:** Callers: `app_init.rs:418` →
`load_rules_ini(&asset_manager, Some(&map_data.ini))` (confirm `map_data` is
in scope above line 418 — it is; line 430 already reads it).
`app.rs:2376` (startup shell, no map selected) →
`.and_then(|am| crate::app_init_helpers::load_rules_ini(am, None))`.

**Step 3: Verify** — `cargo check -p vera20k`;
`rg -n "load_rules_ini\(" src/` shows exactly the def + two updated callers.

**Step 4: Commit** — `app: RC-1 T9 — map INI rules overrides merged into the rules chain at load (rules -> rulesmd -> map)`

### Task 10: AT-9 + AT-10 — end-to-end override and precedence pins

**Why:** Proves the triple-merge reaches `RuleSet` values a sim path consumes,
and pins YR-over-RA2 precedence that was never regression-tested.

**Files:**
- Modify: `src/app_init_helpers.rs` (new `#[cfg(test)] mod tests` at file end)

**Step 1:**

```rust
#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    const RULES_BASE: &str = "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\n\
        [General]\nBuildSpeed=.7\n[CombatDamage]\nC4Delay=.03\n";

    /// AT-9: a map embedding [General]/[CombatDamage] overrides lands those
    /// values in RuleSet, including a sim-consumed path (C4 delay ticks).
    #[test]
    fn map_ini_overrides_rules_values() {
        let mut ini = IniFile::from_str(RULES_BASE);
        let map = IniFile::from_str(
            "[Basic]\nName=Fixture\n[General]\nBuildSpeed=1\n[CombatDamage]\nC4Delay=.06\n");
        ini.merge_rules_overrides(&map);
        let rules = RuleSet::from_ini(&ini).expect("parse");
        // C4Delay is minutes: ticks = minutes * 60 * 15 (ruleset.rs, verified
        // this session) => .06 -> 54.
        assert_eq!(rules.c4_delay_ticks, 54);
        // BuildSpeed consumer — assert the deterministic x1000 field, not the
        // f32 mirror: map override 1 -> 1000 (base .7 would be 700).
        assert_eq!(rules.production.build_speed_x1000, 1000);
    }

    /// AT-9 inverse: a map with no rules-shaped sections is a no-op.
    #[test]
    fn map_without_overrides_leaves_rules_unchanged() {
        let mut with_map = IniFile::from_str(RULES_BASE);
        let map = IniFile::from_str("[Basic]\nName=Clean\n[Waypoints]\n0=45035\n");
        with_map.merge_rules_overrides(&map);
        let a = RuleSet::from_ini(&with_map).expect("parse");
        let b = RuleSet::from_ini(&IniFile::from_str(RULES_BASE)).expect("parse");
        assert_eq!(a.c4_delay_ticks, b.c4_delay_ticks);
        assert_eq!(a.production.build_speed_x1000, b.production.build_speed_x1000);
        assert_eq!(
            a.object("E1").map(|o| o.strength),
            b.object("E1").map(|o| o.strength)
        );
    }

    /// AT-10: a key present in both rules.ini and rulesmd.ini resolves to the
    /// rulesmd value; a rules.ini-only key survives the merge.
    #[test]
    fn rulesmd_overrides_rules_base() {
        let mut ini = IniFile::from_str("[General]\nBuildSpeed=.7\nFlightLevel=1500\n");
        let patch = IniFile::from_str("[General]\nBuildSpeed=.58\n");
        ini.merge(&patch);
        assert_eq!(ini.section("General").unwrap().get("BuildSpeed"), Some(".58"));
        assert_eq!(ini.section("General").unwrap().get("FlightLevel"), Some("1500"));
    }
}
```

If `c4_delay_ticks`'s minutes→ticks conversion differs from 900 ticks/min,
read the actual constant in `ruleset.rs` (do not edit) and fix the expected
value — the assertion must encode the real conversion.

**Step 2: Verify** — `cargo test -p vera20k app_init_helpers` → PASS.

**Step 3: Commit** — `test(app): RC-1 T10 — AT-9 map override reaches RuleSet + consumed C4 ticks; AT-10 rulesmd-over-rules precedence pinned`

### Task 11: RC-1 closeout

**Step 1:** `cargo test -p vera20k` full + `cargo clippy -p vera20k`; all green
(parallel-session file failures excepted, reported not fixed).
**Step 2: Commit** outstanding —
`rules: RC-1 complete — maps can override rules values (registry allocation stays off pending the BLOCKED Read_INI-pass question)`

### Task 12: SC-2 GATE + full `ScenarioSession` definition

**Why:** SC-2 starts here; the struct must exist in final shape so the bincode
layout changes exactly once.

**GATE (mandatory):** `git status --short` — if any of
`src/rules/ruleset.rs`, `src/rules/object_type.rs`, `src/sim/game_entity.rs`,
`src/sim/world/world_spawn.rs` is modified, STOP. Report that SC-1 + RC-1 are
complete and SC-2 awaits the parallel radiation session landing.

**Files:**
- Modify: `src/sim/scenario_session.rs`

**Step 1: Grow the descriptor** (new fields after `seed`, all `pub`):

```rust
    /// Scenario identity: the selected map file name (lobby record / loading
    /// request), with the map's [Basic] Name as a human-facing fallback.
    pub map_name: String,
    /// Theater name from the map [Header] (e.g. "TEMPERATE").
    pub theater: String,
    /// Full map Size= (3rd/4th values) — authoritative bounds at load.
    pub map_width: u16,
    pub map_height: u16,
    /// Playable-area LocalSize= rect.
    pub local_left: u16,
    pub local_top: u16,
    pub local_width: u16,
    pub local_height: u16,
    /// MP start waypoints (index -> cell), from the map [Waypoints] 0..=7.
    /// BTreeMap: deterministic iteration for hashing; sized for the 30-player
    /// target, never assume 8.
    pub mp_start_waypoints: std::collections::BTreeMap<u32, (u16, u16)>,
```

**Step 2: The session aggregate** (same file):

```rust
/// The sim-resident session aggregate — the ScenarioClass analog. Owns
/// session identity, the seed, authoritative map bounds, the MP start table,
/// the per-match options, and the frame clock. Constructed once from the
/// descriptor; serialized and hashed (lockstep state, set before tick 0).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScenarioSession {
    /// Construction seed — the negotiated per-match value; the replay header
    /// records it. Stored widened (the negotiated value is 32-bit).
    pub seed: u64,
    pub map_name: String,
    pub theater: String,
    pub map_width: u16,
    pub map_height: u16,
    pub local_left: u16,
    pub local_top: u16,
    pub local_width: u16,
    pub local_height: u16,
    pub mp_start_waypoints: std::collections::BTreeMap<u32, (u16, u16)>,
    /// Start waypoint index -> owning house, filled during launch application
    /// (after the random-assignment draws), before tick 0.
    pub start_slot_houses: std::collections::BTreeMap<u32, crate::sim::intern::InternedId>,
    /// Per-match game settings (moved from `Simulation.game_options`).
    pub game_options: crate::sim::game_options::GameOptions,
    /// Frame clock (moved from loose `Simulation` fields; commit semantics
    /// unchanged — late-committed at the end of advance_tick).
    pub tick: u64,
    pub total_sim_ms: u64,
    pub binary_frame: u32,
}

impl ScenarioSession {
    pub fn from_descriptor(desc: &ScenarioDescriptor) -> Self {
        Self {
            seed: u64::from(desc.seed),
            map_name: desc.map_name.clone(),
            theater: desc.theater.clone(),
            map_width: desc.map_width,
            map_height: desc.map_height,
            local_left: desc.local_left,
            local_top: desc.local_top,
            local_width: desc.local_width,
            local_height: desc.local_height,
            mp_start_waypoints: desc.mp_start_waypoints.clone(),
            start_slot_houses: std::collections::BTreeMap::new(),
            game_options: crate::sim::game_options::GameOptions::default(),
            tick: 0,
            total_sim_ms: 0,
            binary_frame: 0,
        }
    }
}
```

**Step 3: Verify** — `cargo check -p vera20k` (struct unused yet — allow the
dead-code warning or `pub` exposure silences it).

**Step 4: Commit** — `sim: SC-2 T12 — full ScenarioSession aggregate + descriptor identity/bounds/waypoint fields (not yet owned by Simulation)`

### Task 13: SC-2 field move — hash-neutral (AT-7)

**Why:** The structural heart of SC-2: `Simulation` hands `seed`, the frame
clock, and `GameOptions` to the session WITHOUT shifting a single hash.

**Files:**
- Modify: `src/sim/world/mod.rs`, `src/sim/world/world_hash.rs`,
  `src/sim/snapshot.rs`, `src/app_sim_tick.rs`, plus every file the compiler
  flags (~365 refs; now includes `world_spawn.rs:831` — gate already passed).

**Step 1:** In `Simulation`: delete fields `tick`, `total_sim_ms`,
`binary_frame`, `seed`, `game_options`; add `pub session: ScenarioSession`.
In `with_seed`, build
`session: ScenarioSession::from_descriptor(&ScenarioDescriptor { seed: seed as u32, ..Default::default() })`
— then overwrite `session.seed = seed;` so u64 test seeds (e.g.
`HARNESS_SEED = 0xC0FFEE_1234`, wider than u32) keep their exact recorded
value and the replay-header/seed guards stay byte-stable. In
`from_descriptor`, build the session from the full descriptor, then seed
streams identically to `with_seed`. `reseed_both` sets `self.session.seed`.

**Step 2:** Compiler-driven rename: `cargo check -p vera20k 2>&1 | head -100`,
fix every `E0609`/missing-field error by routing through `self.session.…` /
`sim.session.…`. Do NOT blind-sed: other structs have their own `tick` fields.
Known hot spots: clock commit (`mod.rs:1948`), `world_hash.rs:66-68` +
`hash_game_options` (`:133-134`), `snapshot.rs:120`, `app_sim_tick.rs:263`
(seed) and the `game_options` consumers (~60), `world_spawn.rs:831`.

**Step 3: Hash-fold order is sacred.** `state_hash` keeps the exact sequence:
`session.tick`, `session.total_sim_ms`, `session.binary_frame`, three streams,
… , `hash_game_options` (reading `session.game_options`) at its current
position. NOTHING new is hashed in this task — identity fields stay unhashed.

**Step 4:** `SNAPSHOT_VERSION` 20→21 with this comment appended to the
history block:

```rust
// Bumped 20 -> 21: ScenarioSession — seed/frame-clock/GameOptions move under
// Simulation.session and the session identity fields (map name, theater,
// bounds, MP start waypoints, slot->house) are serialized; the move itself is
// hash-neutral (AT-7), the identity fields fold into the hash in the same
// slice (documented on the golden-harness constant).
```

**Step 5: Verify AT-7** — `cargo test -p vera20k global_skirmish_replay`:
`GLOBAL_HARNESS_FINAL_HASH` must still be `669004916847079430`. If it shifted,
the move was not neutral — find the reordered/changed fold; do NOT re-baseline.
Then full `cargo test -p vera20k`.

**Step 6: Commit** — `sim: SC-2 T13 — seed/frame-clock/GameOptions move into Simulation.session, hash-neutral (golden baseline unshifted); SNAPSHOT_VERSION 20->21`

### Task 14: SC-2 population — identity, bounds, waypoints reach the sim (AT-4/AT-5)

**Why:** Makes the session real: identity and bounds authoritative at
construction instead of app-resident or lazily derived.

**Files:**
- Modify: `src/app_init.rs`, `src/app_init_helpers.rs` (descriptor build),
  `src/app_skirmish.rs` (slot→house fill), `src/sim/vision/mod.rs`,
  `src/app_sim_tick.rs` (header from session),
  `src/sim/scenario_session.rs` (tests)

**Step 1: Build the full descriptor** in `app_init.rs` (extends Task 2's
block; map data is in scope):

```rust
let scenario_descriptor = crate::sim::scenario_session::ScenarioDescriptor {
    seed: crate::app_init_helpers::generate_match_seed(),
    map_name: /* the loading request's map file name; fall back to
                 map_data.basic.name.clone().unwrap_or_default() */,
    theater: map_data.header.theater.clone(),
    map_width: map_data.header.width as u16,
    map_height: map_data.header.height as u16,
    local_left: map_data.header.local_left as u16,
    local_top: map_data.header.local_top as u16,
    local_width: map_data.header.local_width as u16,
    local_height: map_data.header.local_height as u16,
    mp_start_waypoints: crate::map::waypoints::multiplayer_start_waypoints(&map_data.waypoints)
        .into_iter()
        .map(|wp| (wp.index, (wp.rx, wp.ry)))
        .collect(),
};
```

(Find the map file name the init fn already has — the loading request /
`selected_map_file` plumbing; read `app_init.rs` params and use what exists.
Data source must be the map file + lobby record, never a literal.)

**Step 2: Fog bounds at construction.** In `Simulation::from_descriptor`,
after building the session: set `fog.width = session.map_width;
fog.height = session.map_height;` (full Size= — matches what `resolve_bounds`
derives from the PathGrid today; verify by comparing one map's PathGrid dims
against its header Size= at execution and prefer whichever the PathGrid uses,
since AT-7 already proved the lazy value = grid dims). In
`recompute_owner_visibility_in_place` (`vision/mod.rs:464`):

```rust
// Construction-seeded bounds are authoritative; the lazy derivation stays
// only as the fallback for fixture sims built without a descriptor.
let (width, height) = if fog.width > 0 && fog.height > 0 {
    (fog.width, fog.height)
} else {
    resolve_bounds(entities, path_grid)
};
```

**Step 3: Slot→house fill** in `apply_skirmish_launch_session`
(`app_skirmish.rs`, after `let assignments = assign_launch_starts(…)` at
`:198`):

```rust
sim.session.start_slot_houses.clear();
for (slot_idx, waypoint) in &assignments {
    if let Some(slot) = slots.get(*slot_idx) {
        let owner = sim.interner.intern(&slot.owner_name);
        sim.session.start_slot_houses.insert(waypoint.index, owner);
    }
}
```

**Step 4: Replay header derives from the session** (`app_sim_tick.rs:260-266`):
`map_name: sim.session.map_name.clone()` (drop `state.theater_name` here),
`seed: sim.session.seed` (already moved in Task 13).

**Step 5: Tests** (in `scenario_session.rs`):

```rust
/// AT-5: bounds are known before any advance_tick.
#[test]
fn map_bounds_known_before_first_tick() {
    let desc = ScenarioDescriptor {
        seed: 7, map_width: 80, map_height: 60, ..Default::default()
    };
    let sim = Simulation::from_descriptor(&desc);
    assert_eq!((sim.fog.width, sim.fog.height), (80, 60));
}

/// AT-4 (sim-resident identity + snapshot round-trip).
#[test]
fn scenario_identity_is_sim_resident() {
    let desc = ScenarioDescriptor {
        seed: 9, map_name: "tournamentb.map".into(), theater: "SNOW".into(),
        map_width: 100, map_height: 100,
        mp_start_waypoints: [(0u32, (10u16, 12u16)), (1, (88, 90))].into_iter().collect(),
        ..Default::default()
    };
    let sim = Simulation::from_descriptor(&desc);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 1, 2, "tournamentb.map", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes).expect("load").sim;
    assert_eq!(restored.session.map_name, "tournamentb.map");
    assert_eq!(restored.session.theater, "SNOW");
    assert_eq!(restored.session.mp_start_waypoints, sim.session.mp_start_waypoints);
}
```

**Step 6: Verify** — `cargo test -p vera20k scenario_session` +
`cargo test -p vera20k vision` → PASS. Golden baseline STILL unchanged
(nothing new hashed yet).

**Step 7: Commit** — `sim+app: SC-2 T14 — session identity/bounds/waypoints populated at construction; fog bounds authoritative pre-tick; replay header derives from session`

### Task 15: SC-2 hash fold + AT-6

**Why:** The new session fields are lockstep state (setup-phase deficit fill
draws against waypoint state); unhashed they hide desyncs.

**Files:**
- Modify: `src/sim/world/world_hash.rs`,
  `src/sim/world/global_parity_harness_tests.rs`,
  `src/sim/scenario_session.rs` (test)

**Step 1: New fold, appended LAST** in `state_hash` (after
`hash_particle_systems`, so the existing prefix order is untouched):

```rust
self.hash_session_identity(&mut hasher);
```

```rust
/// Session identity/bounds/waypoints — folded after the legacy fields so the
/// pre-session hash prefix order is preserved. Appended in SC-2; order is
/// part of the hash contract and must never change.
fn hash_session_identity(&self, hasher: &mut impl Hasher) {
    let s = &self.session;
    s.seed.hash(hasher);
    s.map_name.hash(hasher);
    s.theater.hash(hasher);
    (s.map_width, s.map_height).hash(hasher);
    (s.local_left, s.local_top, s.local_width, s.local_height).hash(hasher);
    s.mp_start_waypoints.len().hash(hasher);
    for (idx, cell) in &s.mp_start_waypoints {
        idx.hash(hasher);
        cell.hash(hasher);
    }
    s.start_slot_houses.len().hash(hasher);
    for (idx, owner) in &s.start_slot_houses {
        idx.hash(hasher);
        owner.hash(hasher);
    }
}
```

**Step 2: Re-baseline the golden harness ONCE.** Run
`cargo test -p vera20k global_skirmish_replay` — it fails with the new final
hash; update `GLOBAL_HARNESS_FINAL_HASH` and append to its doc comment:

```rust
/// SC-2 re-baseline: session identity fields (seed, map name, theater,
/// bounds, MP start waypoints, slot->house) folded into the hash — every
/// hash shifts once, by design; per-tick rec-vs-replay equality and the
/// per-stream pins prove no behavioral movement.
```

**Step 3: AT-6 test** (`scenario_session.rs`):

```rust
#[test]
fn mp_waypoints_round_trip_and_hash() {
    let mut desc = ScenarioDescriptor {
        seed: 11, map_width: 64, map_height: 64,
        mp_start_waypoints: [(0u32, (5u16, 5u16)), (1, (50, 50))].into_iter().collect(),
        ..Default::default()
    };
    let a = Simulation::from_descriptor(&desc);
    desc.mp_start_waypoints.insert(1, (50, 51)); // one waypoint differs
    let b = Simulation::from_descriptor(&desc);
    assert_ne!(a.state_hash(), b.state_hash(),
        "a one-cell waypoint difference must be visible to the desync detector");

    let bytes = crate::sim::snapshot::GameSnapshot::save(&a, 1, 2, "wp", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes).expect("load").sim;
    assert_eq!(restored.state_hash(), a.state_hash());
}
```

**Step 4: Verify** — `cargo test -p vera20k` full; green.

**Step 5: Commit** — `sim: SC-2 T15 — AT-6 session identity folded into state hash; golden baseline re-baselined once (documented)`

### Task 16: SC-2 closeout — full verification against gamemd contract

**Why:** Confirm the slice satisfies the contract rows before declaring SC-2
done.

**Verify:**
- `cargo test -p vera20k` full + `cargo clippy -p vera20k` — green.
- AT checklist against the contract: AT-1..AT-7 + AT-9/AT-10 each maps to a
  passing named test (list them in the commit body).
- Boundary spot-check: `rg -n "use crate::(render|ui|sidebar|audio|net)" src/sim/scenario_session.rs`
  → no hits.
- gamemd cross-check (no new Ghidra needed): behaviors implemented match the
  contract's verified rows — seed fixed before setup-phase draws (study §2.5),
  bounds/waypoints lockstep-resident pre-tick (study §2.4), map override
  value-only (report §5/§9.3 BLOCKED respected).

**Commit** — `sim: SC-2 complete — ScenarioSession aggregate owns seed/clock/options/identity/bounds/waypoints (contract 2026-06-10, slices SC-1+RC-1+SC-2)`

Optionally append an implementation-status note to the contract doc
(`docs/` is local-only — no git step for it).

---

## Deferred Follow-Ups (explicitly NOT in this plan)

- **RC-3 / AT-11** — table-driven audit of every ported default against
  `RULESCLASS_CONSTRUCTOR_DEFAULTS.csv`. Needs `ruleset.rs` edits → blocked on
  the parallel session; schedule as its own slice.
- **RC-4 / AT-12** — resolution-order fixtures (forward refs, case-duplicate
  names, registry-listed-but-sectionless). Same file ownership; own slice.
- **RC-7 / AT-13** — `speed_multiplier_clamps_at_one` regression pin
  (`terrain_rules.rs:66-73`; check whether a test already exists) + the
  Winged-ignores-INI detail when air-over-terrain costs are next touched.
- **RC-6** — LANGRULE.INI retail-MIX inspection (`/re-investigate`).
- **RC-1 BLOCKED** — map-INI type *allocation* semantics (`/re-investigate`
  the second `Read_INI` pass, report §9.3).
- **RC-8** — doc patch of [RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) §8 + offset comments in
  `game_options.rs` (file not dirty, but cosmetic — bundle with RC-3).
- **Value-sensitive `rules_hash`** — pre-existing gap surfaced during
  planning (`app_sim_tick.rs:1356` hashes registries only).
- **SC-6 / SC-7** — blocked per contract (random-map generator; cell-action
  timer).

**Closeout (2026-06-10, later session):** RC-3/RC-4/RC-7/RC-8, the RC-1 BLOCKED
allocation sub-question, the empty-value DEFERRED item, and RC-6 are all closed
— see the contract's "Follow-up closeout" appendix for details. Net code
changes: empty-value skip in `merge_rules_overrides`; RC-3 default flips to the
verified ctor values (FlightLevel 500, GrowthRate 2.0, RepairStep 5,
RepairPercent 25%, BuildSpeed 1.0; VeteranSight/GapRadius excluded with reason);
three new acceptance tests (AT-11/AT-12/AT-13) + an empty-value regression test.
Still open by design: map-INI TypeClass/`[Colors]` allocation (verified
possible; own feature plan), the value-sensitive `rules_hash` gap, SC-6/SC-7.

## Sources & References

- **Design doc / contract:** `docs/contracts/2026-06-10-scenarioclass-rulesclass-engine-substrate-implementation-contract.md`
- **Ghidra reports (via contract; no new RE this session):**
  [docs/research/RANDOM_SCENARIO_ENGINE_SUBSTRATE_SERVICE_STUDY.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RANDOM_SCENARIO_ENGINE_SUBSTRATE_SERVICE_STUDY.md),
  [docs/research/RULESCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/RULESCLASS_GHIDRA_REPORT.md) (+ defaults CSVs),
  [docs/research/SESSIONCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SESSIONCLASS_GHIDRA_REPORT.md),
  [docs/research/SCENARIOCLASS_PERTICKUPDATE_FRAME_TIMERS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SCENARIOCLASS_PERTICKUPDATE_FRAME_TIMERS_GHIDRA_REPORT.md)
- **gamemd.exe addresses (kept here, never in Rust comments):**
  `Init_Random_Number_System @ 0x0052FC20`, `Start_Scenario @ 0x00683AB0`,
  `RulesClass::Process @ 0x006686C0`, `Read_INI @ 0x00668BF0`,
  `ReadMultiplayerDialogSettings @ 0x00671EA0`,
  `Gather_Start_Positions @ 0x00688380`
- **INI keys:** map `[Header]` Size/LocalSize, map `[Basic]` Name, map
  `[Waypoints]` 0..=7, `[General]` BuildSpeed, `[CombatDamage]` C4Delay,
  `[MultiplayerDialogSettings]` (full key set in `game_options.rs`)
- **Related code (anchors verified this session):** `src/sim/world/mod.rs:85,299-335,488-577,627-634,1948`,
  `src/sim/world/world_hash.rs:63-101,133-154`, `src/sim/rng.rs:25-51`,
  `src/sim/replay.rs:16-92`, `src/sim/snapshot.rs:41,109-141`,
  `src/app_init_helpers.rs:247-289,388-410`, `src/app_init.rs:418,573`,
  `src/app.rs:2376`, `src/app_sim_tick.rs:258-266,1356-1363`,
  `src/app_skirmish.rs:166-202`, `src/skirmish_launch.rs:286-366`,
  `src/rules/ini_parser.rs:304-324`, `src/map/map_file.rs:105-120,156-199`,
  `src/map/waypoints.rs:19,54-62`, `src/sim/vision/mod.rs:455-553`,
  `src/sim/world/global_parity_harness_tests.rs:33-44`
- **Prior commits:** RNG-parity slices (two-stream + mapgen, shipped),
  `af87002d` (SNAPSHOT_VERSION 19→20 precedent), `7b79a186` (golden-baseline
  ceremony precedent)

---

## Execution Log (2026-06-10)

All three slices landed on `dev` the same day the plan was written, interleaved
with a parallel session's radiation (86b0d4bf) and playfield-diamond (7044fcec)
slices. Deviations from the plan as written:

- **SNAPSHOT_VERSION went 21 -> 22**, not 20 -> 21: the radiation slice took 21
  first. The version-pin test became `snapshot_version_is_22`.
- **AT-8 chunked the replay** through the real `ReplayRunner::run` (4 chunks at
  the checkpoint ticks) instead of a manual loop, preserving the live-path
  guarantee. The absolute "mapgen stays zeroed" assert was dropped as planned —
  zero-state draws DO advance the index words (`rng.rs:128-140`); equality pins
  record-vs-replay instead.
- **`PlayfieldBounds` coexistence**: the parallel playfield slice put a verbatim
  `LocalSize` lens on `Simulation.playfield_bounds` (serialized, unhashed).
  `ScenarioSession` owns the hashed authoritative copy; consolidation noted in
  the session doc comment as a follow-up.
- **AT-2 fixture caught a real semantic**: once `map_name` joined the hash,
  playback had to reconstruct the SAME session identity the recording used —
  the fixture now sets `map_name` on the descriptor like the real launch path
  (header derives from session).
- **Re-baselines (one time, documented on the constants):**
  `GLOBAL_HARNESS_FINAL_HASH` 669004916847079430 -> 1003764050363811318;
  `SLICE6_BASELINE_HASH` 11204055998814135587 -> 6388957604790883389.
- Commits: 13fff028, cee3b7d7 (SC-1 T1-T2), 365f1c4a (SC-1 T3-T6),
  30248d38 (RC-1 T8), be9671ef (RC-1 T9-T10), a74be643 (SC-2 T12-T13),
  844c9fbc (SC-2 T14-T15). Full suite green at each commit
  (3842 -> 3854 tests).

## Post-Implementation Adversarial Review (2026-06-10, 18-agent workflow)

Five-lens review + per-finding adversarial verification confirmed 9 findings;
all fixed in `f6a85c4a` except one deferred:

- **BLOCKER (fixed):** session bounds carried raw `[Map] Size=` — the wrong
  coordinate frame (sim cells live in the iso array, ~(W+H) per axis; retail
  Dustbowl: Size=70x76 vs grid 146x146, MP starts at (70,116)/(79,34) outside
  the Size window → player's own base permanently shrouded on every real-map
  launch). Plan Task 14 Step 2 ordered exactly this verification and execution
  skipped it. Fixed: descriptor bounds now come from the resolved cell-array
  dims; raw Size= stays on `playfield_bounds`. Guards: launch-time tripwire
  (start waypoints must be inside bounds) + `tests/session_bounds_frame.rs`
  (retail-map sweep pinning the frame difference).
- **AT-8 (fixed):** record-vs-replay stream equality cannot catch a
  deterministic misroute (both passes share it). Added absolute committed
  per-stream fingerprints at tick 599 (`FINAL_STREAM_STATES`) with the
  re-baseline ceremony. Notable: the golden scenario consumes ZERO gameplay-
  stream draws (scenario == main fingerprint), so any future draw shifts a
  pin loudly.
- **RC-1 (fixed):** `[Particles]`/`[ParticleSystems]` added to the exclusion
  list (mixed-key sections slipped the numeric guard); `[Colors]` (name-keyed
  iterated registry) now merges existing keys only — a new key would allocate
  a scheme, and allocation-from-map is contractually OFF.
- **Docs (fixed):** two stale field paths (`Simulation.total_sim_ms`,
  `Simulation::binary_frame`) updated to the session paths.
- **DEFERRED (needs Ghidra):** a map override with an empty/garbage value
  (`BuildSpeed=`) resets the key to the hardcoded Rust default — neither the
  map's definition nor the merged rulesmd value. gamemd's behavior for
  present-but-empty entries in the map Read_INI pass is UNCHECKED (live check
  showed ReadDouble returns the LIVE field on entry-absent; the empty-value
  branch of INIClass::Load is unverified). One `/re-investigate` session, then
  either skip-empty-keys in `merge_rules_overrides` or pin the verified
  behavior.
