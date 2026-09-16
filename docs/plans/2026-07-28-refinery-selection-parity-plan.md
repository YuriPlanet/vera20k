# Refinery Selection Parity Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained.

**Goal:** Make full-miner refinery selection match retail gamemd: candidates come from the miner's own house only, the normal (narrow) pass rejects refineries whose contact slots are saturated, and a wide fallback pass admits saturated ones only when nothing free exists.

**Architecture:** All changes live inside `sim/miner/` — the selection helper `find_nearest_refinery` (miner_system.rs) and one new read-only probe on `RefineryDockContacts` (miner_dock.rs). The dock-reservation registry stays the admission authority (Slice 8 retirement untouched); no new hashed state, no RNG, no snapshot bump.

**Design Doc:** none (approved inline 2026-07-28: own-house filter + narrow-pass free-slot predicate + wide-pass fallback; user: "ok write plan the review then implement"). Grounding below substitutes for the brainstorm doc's Architecture Context / Impact Analysis.

---

## Grounding Summary

- [docs/research/miner/FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md) (2026-07-28): `FootClass::Find_Docking_Bay` 0x004DF040 → `FUN_004DEE80` scans **only the miner's own house's** BuildingInstances (no alliance loop); distance = 2D squared leptons (Z ignored); per-candidate narrow check `FUN_0065ADF0` is a reservation/contact-list probe **bypassed when the wide-pass flag = 1**; `IsPrimaryFactory` (+0x3D3) overrides distance unconditionally. Spot-checked this session (`MOV ECX,ESI` at 0x004DF07E; COL walk).
- `docs/scans/trace-swarm-20260728/refinery-contact-list.md`: contact array/capacity at building +0xE4/+0xE8; capacity = `NumberOfDocks` floor 1 set once at construction; probe = free-slot-or-already-tracked.
- `docs/scans/trace-swarm-20260728/dock-widescan-global.md`: the wide pass elevates 0x00A8E7AC so `BuildingClass::Receive_Radio` case 0xF accepts already-reserved refineries — i.e. narrow pass rejects contested docks, wide pass admits them. Mission_Harvest state 2 runs narrow first, wide only when narrow found nothing (mission-harvest-cadence.md §3 state 2).
- Repo pattern: `find_nearest_refinery` (src/sim/miner/miner_system.rs:1245-1290) with 3 callers (handle_return :791, handle_forced_return :910, begin_return :1013); capacity lookup pattern `refinery_dock_capacity_for_sid` (:1312-1325, `number_of_docks.max(1)`); registry `RefineryDockContacts` (src/sim/miner/miner_dock.rs:36-68, `hello_or_wait` already implements exactly the free-slot-or-already-tracked shape mutably).
- INI: `NumberOfDocks` already parsed (`ObjectType::number_of_docks`); no new keys.
- Unknown/deferred: `IsPrimaryFactory` override (engine has no primary-designation on refineries yet); native's narrow probe is a radio transmit per candidate (side-effect-free in our model); native distance is 2D lepton² on building coords vs our 2D cell distance on the dock cell (tie-order residual).

## Key Technical Decisions

- Same-owner filter replaces `are_houses_friendly` in `find_nearest_refinery`: **Confidence: high** — Source: FIND_DOCKING_BAY_INTERNALS report (own-house BuildingInstances scan), spot-checked.
- Narrow-pass predicate = `has_contact(ref, miner) || contacts_len < NumberOfDocks`, read-only, via a new `RefineryDockContacts::would_admit`: **Confidence: high** — Source: refinery-contact-list.md contract + existing `hello_or_wait` logic (:53-67) it mirrors immutably.
- Two passes inside one `find_nearest_refinery` call (narrow then wide), callers unchanged except passing `miner_sid`: **Confidence: high** — Source: mission-harvest-cadence.md state 2 (close pass → wide pass in one dispatch).
- Keep existing eligibility gates (dying, health==0, building_up, `is_refinery_type`, `harvester_can_dock_at`): **Confidence: medium** — native eligibility chain not 1:1 audited against these; they are the established Rust gates and none contradicts the reports. Flagged for /review-plan.
- Keep 2D cell distance on the dock cell (not lepton² on building coords): **Confidence: medium** — both are 2D/Z-ignoring; only equidistant tie order can differ. Exactification residual, documented, not implemented. Flagged for /review-plan.

## Open Questions

### Resolved During Planning
- Does any existing test pin allied-refinery acceptance? — No (`rg "allied|ally|are_houses_friendly"` over miner tests: no selection assertions).
- Does the harness shift? — No RNG is consumed by selection and the harness scenario has no refinery; pins unaffected. (Verified during the cadence slice: harness harvester never docks.)

### Deferred to Implementation
- Whether any dock-sequence test implicitly depends on cross-house selection (found only by running the suite).

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `src/sim/miner/miner_dock.rs` | Add read-only `would_admit` probe |
| Modify | `src/sim/miner/miner_system.rs` | Two-pass own-house `find_nearest_refinery`; thread `miner_sid` from the 3 call sites |
| Modify | `src/sim/miner/miner_tests.rs` | 4 new tests; adjust any fallout |

## Interface Changes

- `RefineryDockContacts::would_admit(&self, refinery_sid, miner_sid, capacity) -> bool` — new public read-only method; no existing callers affected.
- `find_nearest_refinery` gains a `miner_sid: u64` parameter — private helper; 3 in-file callers updated in the same task.

## Sim Checklist

- [x] No floats — integer/cell math only.
- [x] No new hashed state (registry unchanged; probe is read-only).
- [x] No render/ui/sidebar/audio/net dependencies.
- [x] No tick-order change; selection stays inside the Harvest dispatch.
- [x] Iteration stays `sim.substrate.entities.values()` (BTreeMap order) — deterministic, unchanged.

## Risk Areas

- Miners with NO own refinery but an allied one now return `None` → `WaitNoOre` (native-correct; previously they delivered to the ally). Player-visible change — this is the point, but watch for tests that spawn only cross-house refineries.
- Narrow-pass rejection changes multi-miner distribution; dock-sequence tests with 2+ miners on one refinery may see different target refineries mid-test. Expected fallout class: retargeting, not timing.

## Player-Experience Critical Items

| Task # | Class | Item | Why it matters | Verification |
|--------|-------|------|----------------|--------------|
| 2-3 | MILESTONE-BLOCKING | Ally-credit misrouting | Our miner deposits into an ally's wallet whenever the ally's refinery is nearer — wrong economy every team game | `miner_docking_never_selects_allied_house_refinery` + full suite |
| 2-3 | COMPOUNDING | Dock load-balancing | Retail miners spread to free refineries; ours convoy to the nearest — visible every harvest cycle with 2+ miners | `miner_narrow_pass_skips_full_refinery_picks_farther_free` |
| 3 | COMPOUNDING | Wide fallback | With every dock saturated, retail miners head to the nearest busy refinery and wait — must not idle | `miner_wide_pass_falls_back_to_saturated_refinery` |
| — | EXACTIFICATION-RESIDUAL | Distance metric tie order | 2D cell distance vs native 2D lepton²; differs only for equidistant candidates | Documented in helper comment; no code |
| — | EXACTIFICATION-RESIDUAL | IsPrimaryFactory override | Needs primary designation, absent from engine | Documented; deferred |

---

## Tasks

### Task 1: Add the read-only narrow-pass probe to the registry

**Why:** The selection pass needs to ask "would this refinery admit me?" without mutating contacts; defines the contract before the consumer.

**Files:**
- Modify: `src/sim/miner/miner_dock.rs` (after `has_contact`, ~line 74)

**Pattern:** mirrors `hello_or_wait` (miner_dock.rs:47-68) immutably.

**Step 1: Implementation**
```rust
    /// Read-only narrow-pass availability probe — the native per-candidate
    /// contact check (`Receive_Radio(0xF)` → free-slot-or-already-tracked)
    /// consulted by dock selection BEFORE any HELLO is sent. True when the
    /// miner already occupies a Contacts[] slot or one is free. The wide
    /// fallback pass skips this probe entirely (native: the elevated leniency
    /// counter makes case 0xF accept reserved refineries).
    pub fn would_admit(&self, refinery_sid: u64, miner_sid: u64, capacity: usize) -> bool {
        self.has_contact(refinery_sid, miner_sid)
            || self.contacts.get(&refinery_sid).map_or(0, Vec::len) < capacity.max(1)
    }
```

**Step 2: Unit tests** (in `miner_dock.rs`'s existing `#[cfg(test)] mod tests`, or append one if absent)
```rust
    #[test]
    fn would_admit_free_slot_tracked_and_saturated() {
        let mut c = RefineryDockContacts::default();
        assert!(c.would_admit(1, 10, 1), "empty refinery admits");
        assert_eq!(c.hello_or_wait(1, 11, 1), ContactAdmission::Accepted);
        assert!(!c.would_admit(1, 10, 1), "saturated refinery rejects a stranger");
        assert!(c.would_admit(1, 11, 1), "already-tracked miner is always admitted");
        assert!(c.would_admit(1, 10, 2), "capacity 2 leaves a free slot");
        assert!(c.would_admit(2, 10, 0), "capacity floors at 1 (native NumberOfDocks floor)");
    }
```

**Step 3: Verify** — `cargo test -p vera20k --lib would_admit` → PASS.

**Step 4: Commit** — `miner: read-only would_admit probe on RefineryDockContacts`

### Task 2: Two-pass own-house find_nearest_refinery

**Why:** The core parity change; all three FSM callers route through this one helper.

**Files:**
- Modify: `src/sim/miner/miner_system.rs:1236-1290` (`find_nearest_refinery`) and call sites :791, :910, :1013.

**Pattern:** existing helper structure; capacity lookup mirrors `refinery_dock_capacity_for_sid` (:1312).

**Step 1: Replace the helper** (keep the doc comment position; replace body and comment)
```rust
/// Find the nearest refinery the miner may return to. Returns (stable_id, dock_cell).
///
/// Native contract (Find_Docking_Bay 0x004DF040, FIND_DOCKING_BAY_INTERNALS
/// report 2026-07-28): candidates are the miner's OWN house's buildings only —
/// never an ally's (deposit credits go to the refinery owner, so the old
/// alliance filter misrouted team-game income). The narrow pass rejects a
/// refinery whose Contacts[] are saturated (free-slot-or-already-tracked
/// probe); the wide fallback pass admits saturated ones so a miner heads to
/// the nearest busy dock and waits instead of idling. Distance stays 2D cell
/// distance on the dock cell (native: 2D lepton² on building coords; only
/// equidistant tie order can differ — recorded residual). The native
/// IsPrimaryFactory override is deferred until primary designation exists.
fn find_nearest_refinery(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    harvester_type_id: &str,
    from: (u16, u16),
    miner_sid: u64,
) -> Option<(u64, (u16, u16))> {
    find_refinery_pass(sim, rules, owner, harvester_type_id, from, Some(miner_sid))
        .or_else(|| find_refinery_pass(sim, rules, owner, harvester_type_id, from, None))
}

/// One selection pass. `narrow_probe = Some(miner_sid)` applies the native
/// per-candidate contact check; `None` is the wide pass (probe skipped).
fn find_refinery_pass(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    harvester_type_id: &str,
    from: (u16, u16),
    narrow_probe: Option<u64>,
) -> Option<(u64, (u16, u16))> {
    let mut best: Option<(u32, u64, u16, u16)> = None;
    for entity in sim.substrate.entities.values() {
        let e_owner = sim.interner.resolve(entity.owner);
        let e_type = sim.interner.resolve(entity.type_ref);
        if entity.category != EntityCategory::Structure
            // Native scans only the owner house's BuildingInstances.
            || !e_owner.eq_ignore_ascii_case(owner)
            || !rules.is_refinery_type(e_type)
            || !rules.harvester_can_dock_at(harvester_type_id, e_type)
            // Death animations keep the building entity around, but gamemd
            // calls UndockUnit from damage/sell paths before accepting more cargo.
            || entity.dying
            // TibSun legacy: skip dead buildings (CanDock checks HP > 0).
            || entity.health.current == 0
            // TibSun legacy: skip buildings under construction (CanDock rejects mission 0x13).
            || entity.building_up.is_some()
        {
            continue;
        }
        if let Some(miner_sid) = narrow_probe {
            let capacity = sim
                .object_type(entity.type_ref, rules)
                .map(|o| o.number_of_docks.max(1) as usize)
                .unwrap_or(1);
            if !sim
                .production
                .dock_reservations
                .would_admit(entity.stable_id, miner_sid, capacity)
            {
                continue;
            }
        }
        let obj = rules.object_case_insensitive(e_type);
        let (w, h) = obj
            .map(|o| foundation_dimensions(&o.foundation))
            .unwrap_or((1, 1));
        let qc = obj.and_then(|o| o.queueing_cell);
        let dock = refinery_dock_cell(entity.position.rx, entity.position.ry, w, h, qc);
        let dx = i64::from(dock.0) - i64::from(from.0);
        let dy = i64::from(dock.1) - i64::from(from.1);
        let dist_sq = (dx * dx + dy * dy) as u32;
        match best {
            Some((d, _, _, _)) if dist_sq >= d => {}
            _ => best = Some((dist_sq, entity.stable_id, dock.0, dock.1)),
        }
    }
    best.map(|(_, sid, dx, dy)| (sid, (dx, dy)))
}
```
Note: the `are_houses_friendly` import and the old alliance comment go away; remove the now-unused `crate::map::houses::are_houses_friendly` use if nothing else in the file references it (rg first).

**Step 2: Thread `miner_sid` at the three call sites.** Each currently reads
```rust
        if let Some((rsid, _dock)) = find_nearest_refinery(
            sim,
            rules,
            sim.interner.resolve(snap.owner),
            sim.interner.resolve(snap.type_id),
            (snap.rx, snap.ry),
        ) {
```
Append `snap.entity_id,` as the final argument in `handle_return` (:791), `handle_forced_return` (:910), and `begin_return` (:1013).

**Step 3: Verify** — `cargo check -p vera20k --lib` clean; `cargo test -p vera20k --lib miner` (expect possible fallout — fix in Task 4, do not mask).

**Step 4: Commit** — `miner: own-house two-pass refinery selection (narrow contact probe + wide fallback)`

### Task 3: New behavior tests

**Why:** Pin the three player-visible contracts and the already-tracked idempotency.

**Files:**
- Modify: `src/sim/miner/miner_tests.rs` (append near the other selection/dock tests; use the existing `spawn_miner`/`spawn_refinery`/`miner_rules`/`tick_miners_n`/`get_miner` helpers — `spawn_refinery` spawns for owner "Americans"; check its signature and, if owner is hardcoded, add a `spawn_refinery_for_owner` variant copying it with an owner parameter).

**Step 1: Tests**
```rust
/// Native Find_Docking_Bay scans only the owner house's buildings: an allied
/// refinery is never a return target, even when it is the only/nearest one
/// (FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md 2026-07-28).
#[test]
fn miner_docking_never_selects_allied_house_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    // Allied refinery adjacent, own refinery far.
    spawn_refinery_for_owner(&mut sim, 2, 7, 10, "French");
    sim.house_alliances = alliances_with("Americans", "French");
    spawn_refinery(&mut sim, 3, 40, 10); // own (Americans)
    fill_miner_full(&mut sim, miner_id);
    set_state(&mut sim, miner_id, MinerState::ReturnToRefinery);

    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "own-house refinery must win over a nearer allied one",
    );
}

/// Narrow pass rejects a saturated refinery; the miner picks the farther free
/// one instead of convoying (refinery-contact-list.md contract).
#[test]
fn miner_narrow_pass_skips_full_refinery_picks_farther_free() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10); // near, will be saturated
    spawn_refinery(&mut sim, 3, 30, 10); // far, free
    let cap = 1; // stock NumberOfDocks=1 in miner_rules fixtures
    assert_eq!(
        sim.production.dock_reservations.hello_or_wait(2, 99, cap),
        crate::sim::miner::miner_dock::ContactAdmission::Accepted,
    );
    fill_miner_full(&mut sim, miner_id);
    set_state(&mut sim, miner_id, MinerState::ReturnToRefinery);

    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(3),
        "saturated near refinery must lose to the farther free one",
    );
}

/// Wide fallback: when every own refinery is saturated the miner still targets
/// the nearest one (drive-up-and-wait), never idling (dock-widescan-global.md).
#[test]
fn miner_wide_pass_falls_back_to_saturated_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10);
    assert_eq!(
        sim.production.dock_reservations.hello_or_wait(2, 99, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Accepted,
    );
    fill_miner_full(&mut sim, miner_id);
    set_state(&mut sim, miner_id, MinerState::ReturnToRefinery);

    tick_miners_n(&mut sim, &rules, 1);
    let m = get_miner(&sim, miner_id);
    assert_eq!(m.reserved_refinery, Some(2), "wide pass admits the busy refinery");
    assert_eq!(m.state, MinerState::ReturnToRefinery, "miner keeps returning, not WaitNoOre");
}

/// A miner already in a refinery's Contacts[] passes the narrow probe even at
/// capacity — re-selection must not evict it to a farther refinery.
#[test]
fn miner_already_tracked_keeps_saturated_refinery() {
    let mut sim = Simulation::new();
    let rules = miner_rules();
    let miner_id = spawn_miner(&mut sim, 1, MinerKind::War, 5, 10);
    spawn_refinery(&mut sim, 2, 10, 10);
    spawn_refinery(&mut sim, 3, 30, 10);
    assert_eq!(
        sim.production.dock_reservations.hello_or_wait(2, miner_id, 1),
        crate::sim::miner::miner_dock::ContactAdmission::Accepted,
    );
    fill_miner_full(&mut sim, miner_id);
    set_state(&mut sim, miner_id, MinerState::ReturnToRefinery);

    tick_miners_n(&mut sim, &rules, 1);
    assert_eq!(
        get_miner(&sim, miner_id).reserved_refinery,
        Some(2),
        "already-tracked miner stays with its refinery",
    );
}
```
Helper notes (self-containment): `fill_miner_full`/`set_state` may not exist under those names — inline the established fixture idiom used by `full_miner_losing_dying_refinery_keeps_returning` (miner_tests.rs:6484-6502): push `capacity_bales` × `CargoBale { resource_type: ResourceType::Ore, value: 25 }` and `entity.mission.set_handler_state(MinerState::ReturnToRefinery.cursor())`. For `spawn_refinery_for_owner` and `alliances_with`, copy `spawn_refinery`'s body with an owner argument and build the alliance map the way existing alliance-aware tests in `src/sim/world` do (`sim.house_alliances` insertion); if no precedent exists in miner_tests, keep the allied test minimal: only assert the allied refinery is NOT selected when it is the sole refinery (`reserved_refinery == None` and state falls to `WaitNoOre`), which needs no alliance map at all — a cross-house refinery must simply never match.

**Step 2: Verify** — `cargo test -p vera20k --lib miner_docking_never miner_narrow_pass miner_wide_pass miner_already_tracked` → 4 × PASS.

**Step 3: Commit** — `miner: selection parity tests (own-house, narrow reject, wide fallback, tracked keep)`

### Task 4: Full-suite fallout + retiming

**Why:** Selection changes can retarget miners mid-test in multi-refinery/multi-miner fixtures.

**Steps:**
1. `cargo test -p vera20k --lib 2>&1 | grep -E "FAILED|test result"`.
2. For each failure: decide whether the test's premise assumed cross-house selection or nearest-regardless-of-saturation. Update the fixture (spawn the refinery under the miner's owner / free a slot) rather than weakening the assertion; if the test pinned the OLD behavior as a contract, rewrite its doc comment citing [FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md).
3. Re-run until `0 failed`; record the literal `test result:` line.
4. Commit — `miner: retime/refit tests for own-house narrow-pass selection`

### Task 5: Live verify

**Why:** The change alters which refinery real miners choose; confirm the loop stays visibly normal.

**Verify:**
- Launch the worktree build: `RA2_QUICKPLAY=minerloop.map` from the main repo root (see memory `reference-quickplay-minerloop-live-verify`; the map pre-places an Americans NAREFN+HARV).
- Watch `logs/ra2.log` for the `MINER <id>` transitions: full harvest→ReturnToRefinery→Dock→(cargo→0)→SearchOre cycle, no WaitNoOre stalls.
- Kill the instance afterwards.

## Sources & References

- **Ghidra reports:** [docs/research/miner/FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/miner/FIND_DOCKING_BAY_INTERNALS_GHIDRA_REPORT.md); docs/scans/trace-swarm-20260728/refinery-contact-list.md; docs/scans/trace-swarm-20260728/dock-widescan-global.md; docs/scans/trace-swarm-20260728/mission-harvest-cadence.md §3 (state 2 ordering)
- **gamemd.exe addresses:** Find_Docking_Bay 0x004DF040; per-house scan FUN_004DEE80; contact probe FUN_0065ADF0; Receive_Radio case 0xF 0x0043c2d0; leniency global 0x00A8E7AC — addresses stay here, not in Rust comments.
- **INI keys:** rulesmd.ini per-building `NumberOfDocks=` (already parsed).
- **Related code:** src/sim/miner/miner_system.rs:791,910,1013,1236-1325; src/sim/miner/miner_dock.rs:36-108.
- **Prior commits:** 8c74f28f (cadence slice, same branch).
