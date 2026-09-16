# Sell-on-Captured-Civilian Branch — Implementation Plan

> **For Claude:** Execute this plan task-by-task. Each task is self-contained.

**Goal:** Make `sell_building` eject + revert (without demolishing) when called on a captured civilian building, matching gamemd's `SellBuilding @ 0x00457DE0` semantics on civilian-controlled structures. Resolves v2 disparity-scan finding **N1**.

**Architecture:** Branch inside `sell_building` on `entity.garrison_original_owner.is_some()`. Reuse the existing `eject_garrison_occupants` helper (which already handles eject + cargo clear + conditional owner revert). Push `SimSoundEvent::StructureAbandoned` and early-return BEFORE `entities.remove` and refund. Player-built path stays unchanged.

**Design Doc:** [docs/plans/2026-05-04-unified-garrison-eject-design.md](docs/plans/2026-05-04-unified-garrison-eject-design.md)

> ⚠️ **Scope reduction:** `/review-plan` confirmed that the destruction-eject pipeline (originally Tasks 1, 4, 5, 6 in the prior draft) was already implemented in 5 parallel-session commits on `dev` (`83338d3`, `9db662e`, `f5195c8`, `527b3f4`, `fef8824`). The work uses a deferred-eject architecture (`DestroyedGarrisonBuilding` collected in `CombatTickResult`, consumed by world layer via `eject_destruction_garrison`). It already has test coverage (`test_garrison_eject_on_destruction_happy_path` at `passenger.rs:1048`). This plan now covers ONLY the sell-path branch, which is still unimplemented.

---

## Grounding Summary

- **Docs:** [ra2-rust-game-docs/GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) §14c (SellBuilding ejection); `BUILDING_CHANGE_OWNER_GHIDRA_REPORT.md` (CheckAutoSellOrCivilian / civilian revert semantics).
- **Ghidra:** `BuildingClass::SellBuilding @ 0x00457DE0` ejects occupants atomically; for civilian buildings whose ownership transferred via garrisoning, the structure is preserved (ownership reverts to civilian house). Verified in prior /verify-doc audits — accepted.
- **Repo state:** `sell_building` at `production_sell.rs:508-554` always does unconditional `entities.remove(stable_id)` (line 532). `eject_garrison_occupants` at `production_sell.rs:247-374` already handles eject + cargo clear + conditional owner revert (line 368-370 reverts iff `garrison_original_owner` was `Some`). `garrison_original_owner` is `.take()`'d in `passenger.rs:614` when cargo empties on unload, so `is_some()` ⇔ "captured AND has occupants".
- **Pattern:** `SimSoundEvent::StructureAbandoned` push pattern at `passenger.rs:619-622` (pre-revert owner captured before mut borrow). Existing test pattern `test_last_occupant_emits_abandoned_event_with_pre_revert_owner` at `passenger.rs:864-913` mirrors what we want.
- **Death-path is done:** parallel session implemented destruction-eject as deferred via `combat::CombatTickResult.destroyed_garrison_buildings` → world layer calls `production::eject_destruction_garrison`. Tested at `passenger.rs:1048+`. Out of scope for this plan.
- **INI keys:** `CanBeOccupied`, `MaxNumberOccupants`, `ConditionRed` — already parsed.
- **Unknown:** None.

## Key Technical Decisions

- **Branch lives inside `sell_building`** — not in the validator, not in the cursor. Matches gamemd's "Sell button works on captured civilians, just doesn't demolish" UX. **Confidence:** high. **Source:** disparity-scan + design doc.
- **`is_captured` gate uses `garrison_original_owner.is_some()` alone** — auto-revert at `passenger.rs:614` clears it on cargo-empty, so `is_some()` implies cargo non-empty. No separate cargo check needed. **Confidence:** high. **Source:** passenger.rs:606-622 (verified).
- **Reuse existing `eject_garrison_occupants` helper as-is** — it already ejects atomically, clears cargo, and reverts owner when `garrison_original_owner` is `Some`. No refactor needed. **Confidence:** high. **Source:** production_sell.rs:247-374.
- **Pre-revert owner captured by reading `entity.owner` before calling helper** — helper reverts owner internally. Capturing `entity.owner` before the call gives us the pre-revert value for `StructureAbandoned`. Mirrors pattern at `passenger.rs:606-610`. **Confidence:** high.

## Open Questions

### Resolved During Planning

- Helper refactor needed? → No. Existing helper signature works as-is.
- RNG plumbing into combat? → No. Out of scope (death-path already done elsewhere).
- Cursor changes? → No. gamemd shows Sell cursor; behavior diverges in `sell_building`.

### Deferred to Implementation

- None.

## File Map

| Action | Path | Responsibility |
|--------|------|----------------|
| Modify | `src/sim/production/production_sell.rs:508-554` | Add captured-civilian branch in `sell_building` |
| Modify | `src/sim/production/production_tests.rs:352-400` | Extend `sell_rules()` to include a `CanBeOccupied` building (or add sibling `sell_garrison_rules()` helper) |
| Modify | `src/sim/production/production_placement_tests.rs` | Add 3 tests pinning new + regression behavior |

## Interface Changes

None. `sell_building` signature unchanged. `eject_garrison_occupants` signature unchanged. `SimSoundEvent::StructureAbandoned` already exists.

## Sim Checklist

- [x] All math integer/fixed-point (no new math).
- [x] No new state on `GameEntity`. Existing state hash unaffected.
- [x] No dependencies on render/ui/sidebar/audio/net.
- [x] Tick ordering impact: none.
- [x] BTreeMap iteration order: irrelevant (no entity scan added).
- [x] Determinism: helper already deterministic; `StructureAbandoned` push is deterministic event sink. No new RNG draws.

## Risk Areas

- **Borrowing for pre-revert owner:** must read `entity.owner` and exit the immutable borrow scope before calling `eject_garrison_occupants` (which takes `&mut Simulation`). Mitigate by capturing into a local in the same `let (...) = { ... }` block as the existing snapshot.
- **Refund-bypass for captured civilian:** `sell_refund_for_building` would compute 0 anyway (civilian buildings have `Cost=0`), so even if we accidentally pay the refund the credit delta is 0. But the early-return is cleaner and correct semantically.

## Parity-Critical Items

| Task # | Item | Why it matters | Verification |
|--------|------|----------------|--------------|
| Task 1 | Captured-civilian Sell ejects + reverts WITHOUT demolish | Player loses building permanently otherwise (N1 HIGH) | Test in Task 2 + manual /trace-action |
| Task 1 | Atomic single-tick eject (not multi-tick) | Helper already atomic; preserved by direct call | Existing helper unchanged |
| Task 1 | StructureAbandoned EVA owner is pre-revert | Otherwise EVA fires for "Civilian abandoning Civilian" — wrong owner | Test in Task 2 |
| Task 1 | No refund paid on captured-civilian sell | Civilian buildings have Cost=0 so refund is 0 anyway, but explicit early-return prevents future regressions if Cost changes | Test in Task 2 |

---

## Tasks

### Task 1: Add captured-civilian branch in `sell_building`

**Why:** N1 HIGH — currently selling a garrisoned civilian permanently demolishes the structure and pays a refund the player shouldn't get. gamemd ejects + reverts the structure to civilian without destroying it.

**Files:**
- Modify: `src/sim/production/production_sell.rs:508-554` (`sell_building` function)

**Pattern:** Early-return branch. `StructureAbandoned` push mirrors `passenger.rs:619-622` (pre-revert owner captured before mut borrow).

**Step 1: Extend the snapshot tuple to capture `is_captured` and pre-revert owner**

In `sell_building` at `production_sell.rs:509-523`, change the snapshot block from:

```rust
let (owner_name, type_id, position, health) = {
    let Some(entity) = sim.entities.get(stable_id) else {
        return false;
    };
    if entity.category != EntityCategory::Structure {
        return false;
    }
    (
        sim.interner.resolve(entity.owner).to_string(),
        sim.interner.resolve(entity.type_ref).to_string(),
        entity.position.clone(),
        Some(entity.health),
    )
};
```

To:

```rust
let (owner_name, type_id, position, health, is_captured, abandoning_owner) = {
    let Some(entity) = sim.entities.get(stable_id) else {
        return false;
    };
    if entity.category != EntityCategory::Structure {
        return false;
    }
    (
        sim.interner.resolve(entity.owner).to_string(),
        sim.interner.resolve(entity.type_ref).to_string(),
        entity.position.clone(),
        Some(entity.health),
        entity.garrison_original_owner.is_some(),
        entity.owner,
    )
};
```

**Step 2: Insert captured-civilian early-return branch**

Immediately after the `let Some(obj) = rules.object(&type_id) else { return false; };` line (production_sell.rs:524-526), insert:

```rust
// Captured-civilian branch: eject + revert + KEEP building, no refund.
// Matches gamemd's SellBuilding semantics on civilian buildings whose
// ownership transferred via garrisoning (CheckAutoSellOrCivilian).
// `eject_garrison_occupants` reverts owner internally because
// `garrison_original_owner` is Some.
if is_captured {
    let garrison_ejected = eject_garrison_occupants(sim, rules, stable_id);
    sim.sound_events
        .push(crate::sim::world::SimSoundEvent::StructureAbandoned {
            owner: abandoning_owner,
        });
    log::info!(
        "Building {} evacuated by {}: {} occupants ejected, structure reverted to civilian",
        type_id,
        owner_name,
        garrison_ejected
    );
    return true;
}
```

The existing player-built/non-garrison flow that follows (refund calc, eject_sell_survivors, eject_garrison_occupants, entities.remove, refund payment) is unchanged.

**Step 3: Verify**

Run: `cargo check -p ra2_engine`
Expected: compiles.

Run: `cargo test -p ra2_engine production -- --nocapture`
Expected: existing sell tests still PASS (`sell_building_refunds_half_current_value_and_ejects_allied_infantry`, `sell_building_uses_owner_appropriate_survivor_type_and_caps_count`). Their setups don't set `garrison_original_owner`, so `is_captured` is `false` and they hit the existing path unchanged.

**Step 4: Commit**

Commit message: `garrison: sell on captured civilian ejects + reverts without demolishing`

---

### Task 2: Tests for the new branch + player-built regression

**Why:** Pin the new captured-civilian behavior, the EVA cue owner, and the player-built-garrison demolition (regression — must keep ejecting alive AND demolishing).

**Files:**
- Modify: `src/sim/production/production_placement_tests.rs` (append 3 tests)

**Pattern:** Mirrors the existing tests at `production_placement_tests.rs:944-1024`. Uses existing helpers `spawn_structure`, `sell_rules`, `credits_for_owner`, `super::credits_entry_for_owner`. Helper IDs follow the existing test convention.

**Step 1: Confirm `sell_rules()` includes a `CanBeOccupied` building**

The `sell_rules()` helper lives at `src/sim/production/production_tests.rs:352-400` (it's `pub(super) fn sell_rules` and is imported into `production_placement_tests.rs:23-26` via `use super::tests::{... sell_rules, spawn_structure ...};`). Open `production_tests.rs` and locate `sell_rules()`. If it doesn't already include a `CanBeOccupied=yes` building (e.g., `CAGAS01`), extend it with one. The existing tests use `GAPOWR` and `NAHAND` — neither is garrisonable. Add a section like:

```ini
[BuildingTypes]
... existing ...
N=CAGAS01

[CAGAS01]
Name=GasStation
Cost=0
Strength=400
Armor=wood
CanBeOccupied=yes
CanOccupyFire=yes
MaxNumberOccupants=5
```

Plus an `[InfantryTypes]` entry for `E1` if not already present (with `Occupier=yes Size=1`). Mirror the rules used in `passenger.rs::garrison_test_rules()` at `passenger.rs:634-668` — copy that block verbatim into `sell_rules` if needed.

Verified safe to extend: existing tests `sell_building_refunds_half_current_value_and_ejects_allied_infantry` and `sell_building_uses_owner_appropriate_survivor_type_and_caps_count` use `GAPOWR` and `NAHAND` only and don't enumerate the building list. If you'd rather not touch the shared helper, add a separate `pub(super) fn sell_garrison_rules()` in `production_tests.rs` and import it the same way.

**Step 2: Add test #1 — captured-civilian sell keeps building**

```rust
#[test]
fn sell_captured_civilian_ejects_reverts_and_keeps_building() {
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let mut sim = Simulation::new();
    let rules = sell_rules();  // or sell_garrison_rules() if you split
    *super::credits_entry_for_owner(&mut sim, "Americans") = 1000;

    // Spawn a CanBeOccupied building owned by Americans, with
    // garrison_original_owner = Some(Neutral).
    spawn_structure(&mut sim, 10, "Americans", "CAGAS01", 20, 20);
    let neutral_id = sim.interner.intern("Neutral");
    if let Some(t) = sim.entities.get_mut(10) {
        t.garrison_original_owner = Some(neutral_id);
        t.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 1),
        };
    }
    // Two occupants inside the cargo.
    let amer_id = sim.interner.intern("Americans");
    let e1_id = sim.interner.intern("E1");
    for &pid in &[11u64, 12u64] {
        let mut pax = crate::sim::game_entity::GameEntity::test_default(pid, "E1", "Americans", 19, 20);
        pax.owner = amer_id;
        pax.type_ref = e1_id;
        pax.passenger_role = PassengerRole::Inside { transport_id: 10 };
        sim.entities.insert(pax);
    }
    if let Some(t) = sim.entities.get_mut(10) {
        if let Some(c) = t.passenger_role.cargo_mut() {
            c.board(11, 1);
            c.board(12, 1);
        }
    }

    assert!(sell_building(&mut sim, &rules, 10));

    // Building still in store, owner reverted, cargo cleared.
    let bldg = sim.entities.get(10).expect("building should still exist");
    assert_eq!(sim.interner.resolve(bldg.owner), "Neutral");
    assert!(bldg.garrison_original_owner.is_none(), "original_owner should have been consumed");
    let cargo = bldg.passenger_role.cargo().expect("cargo");
    assert!(cargo.is_empty(), "cargo should be cleared");

    // Both occupants alive on the map, role=None, not dying.
    for &pid in &[11u64, 12u64] {
        let pax = sim.entities.get(pid).expect("occupant exists");
        assert!(!pax.dying, "occupant {pid} should not be dying");
        assert!(pax.health.current > 0, "occupant {pid} should be alive");
        assert!(matches!(pax.passenger_role, PassengerRole::None), "occupant {pid} role should be None");
    }

    // No refund credited.
    assert_eq!(credits_for_owner(&sim, "Americans"), 1000, "captured-civilian sell pays no refund");
}
```

**Step 3: Add test #2 — emits StructureAbandoned with pre-revert owner**

```rust
#[test]
fn sell_captured_civilian_emits_structure_abandoned_with_pre_revert_owner() {
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    use crate::sim::world::SimSoundEvent;
    let mut sim = Simulation::new();
    let rules = sell_rules();  // same helper as test #1
    spawn_structure(&mut sim, 20, "Americans", "CAGAS01", 30, 30);
    let neutral_id = sim.interner.intern("Neutral");
    let amer_id = sim.interner.intern("Americans");
    let e1_id = sim.interner.intern("E1");
    if let Some(t) = sim.entities.get_mut(20) {
        t.garrison_original_owner = Some(neutral_id);
        t.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 1),
        };
    }
    let mut pax = crate::sim::game_entity::GameEntity::test_default(21, "E1", "Americans", 29, 30);
    pax.owner = amer_id;
    pax.type_ref = e1_id;
    pax.passenger_role = PassengerRole::Inside { transport_id: 20 };
    sim.entities.insert(pax);
    if let Some(t) = sim.entities.get_mut(20) {
        if let Some(c) = t.passenger_role.cargo_mut() {
            c.board(21, 1);
        }
    }

    assert!(sell_building(&mut sim, &rules, 20));

    let mut found = false;
    for evt in &sim.sound_events {
        if let SimSoundEvent::StructureAbandoned { owner } = evt {
            assert_eq!(
                sim.interner.resolve(*owner),
                "Americans",
                "StructureAbandoned should carry pre-revert owner (Americans), not post-revert civilian"
            );
            found = true;
        }
    }
    assert!(found, "expected StructureAbandoned event after captured-civilian sell");
}
```

**Step 4: Add test #3 — player-built garrison still demolishes + ejects alive**

This is the regression test guarding the player-built path.

```rust
#[test]
fn sell_player_built_garrisoned_building_demolishes_and_ejects_alive() {
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let mut sim = Simulation::new();
    let rules = sell_rules();
    *super::credits_entry_for_owner(&mut sim, "Americans") = 0;

    // Spawn a CanBeOccupied building OWNED by Americans, NO original_owner
    // (player-built, not captured). Use a building with non-zero Cost so we
    // can verify refund — pick one that exists in sell_rules with Cost>0.
    // If sell_rules's CanBeOccupied building has Cost=0, replace 'CAGAS01'
    // with a player-built garrisonable variant in sell_rules and use that.
    spawn_structure(&mut sim, 30, "Americans", "CAGAS01", 40, 40);
    let amer_id = sim.interner.intern("Americans");
    let e1_id = sim.interner.intern("E1");
    if let Some(t) = sim.entities.get_mut(30) {
        // garrison_original_owner stays None — player-built.
        t.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 1),
        };
    }
    let mut pax = crate::sim::game_entity::GameEntity::test_default(31, "E1", "Americans", 39, 40);
    pax.owner = amer_id;
    pax.type_ref = e1_id;
    pax.passenger_role = PassengerRole::Inside { transport_id: 30 };
    sim.entities.insert(pax);
    if let Some(t) = sim.entities.get_mut(30) {
        if let Some(c) = t.passenger_role.cargo_mut() {
            c.board(31, 1);
        }
    }

    assert!(sell_building(&mut sim, &rules, 30));

    // Building removed.
    assert!(!sim.entities.contains(30), "player-built garrison should be demolished on sell");
    // Occupant placed on the map alive.
    let pax = sim.entities.get(31).expect("occupant exists");
    assert!(!pax.dying, "occupant should not be dying");
    assert!(pax.health.current > 0, "occupant should be alive");
    assert!(matches!(pax.passenger_role, PassengerRole::None), "occupant role should be None");
    // Refund: if CAGAS01 in sell_rules has Cost=0, this assertion is "= 0"
    // (the demolition path simply pays nothing for a Cost=0 building).
    // If you want a positive-refund regression, use a player-built
    // garrisonable building with Cost>0 from sell_rules.
}
```

**Note on the refund assertion:** `CAGAS01` (a civilian gas station) has `Cost=0` in vanilla rulesmd. The existing demolition path pays a 0 refund for Cost=0 buildings; that's correct and pre-existing behavior. The test's value is in pinning the *demolition* (entities.remove fired) and the *occupant ejection alive*, not in the refund magnitude. If the test setup needs a positive-refund garrisonable for stronger coverage, add a fictional `[GASTANK]` entry with `Cost=500 CanBeOccupied=yes` to `sell_rules` and use that instead.

**Step 5: Verify**

Run: `cargo test -p ra2_engine sell_captured_civilian -- --nocapture`
Run: `cargo test -p ra2_engine sell_player_built_garrisoned -- --nocapture`
Expected: 3 new tests PASS.

Run: `cargo test -p ra2_engine -- --nocapture`
Expected: full suite PASS, including existing garrison/sell tests and the parallel-session destruction-eject tests.

**Step 6: Commit**

Commit message: `garrison: tests for sell captured civilian + player-built regression`

---

## Sources & References

- **Design doc:** [docs/plans/2026-05-04-unified-garrison-eject-design.md](docs/plans/2026-05-04-unified-garrison-eject-design.md)
- **v2 disparity scan:** [docs/gap-scans/2026-05-04-disparity-scan-garrison-v2.md](docs/gap-scans/2026-05-04-disparity-scan-garrison-v2.md) — finding N1
- **Ghidra reports:** [ra2-rust-game-docs/GARRISON_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/GARRISON_SYSTEM_GHIDRA_REPORT.md) §14c; `ra2-rust-game-docs/BUILDING_CHANGE_OWNER_GHIDRA_REPORT.md`
- **gamemd.exe addresses:** `BuildingClass::SellBuilding @ 0x00457DE0`; `BuildingClass::CheckAutoSellOrCivilian @ 0x00458200`
- **Related code:** `src/sim/passenger.rs:606-622` (existing StructureAbandoned push pattern); `src/sim/passenger.rs:864-913` (existing pre-revert owner test pattern); `src/sim/production/production_sell.rs:247-374` (`eject_garrison_occupants` helper, unchanged)
- **Parallel-session work (out of scope, already on `dev`):** commits `83338d3` `9db662e` `f5195c8` `527b3f4` `fef8824` (combat death-path eject via `DestroyedGarrisonBuilding` + `eject_destruction_garrison`)
