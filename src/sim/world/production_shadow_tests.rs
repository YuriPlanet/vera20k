//! P1+P2 production+economy shadow tests (study §8 P1/P2 + the design proving set).
//!
//! These exercise the shadow build from the `world` level, where `Simulation::new`,
//! `advance_tick`, `state_hash`, `set_logic_order_for_test`, and the snapshot API
//! are reachable. The contract these pin: the shadow is DERIVED from the legacy
//! state, NEVER creates a house, and leaves `state_hash()` bit-identical (the new
//! fields are `#[serde(skip)]` with no serde derive, so `SNAPSHOT_VERSION` stays 17).

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{CancelOutcome, PRODUCTION_STEPS, ProductionCategory, StepOutcome};
use std::collections::BTreeMap;

fn empty_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str("")).expect("empty rules parse")
}

/// Rules with a costed buildable vehicle (`GRIZZLY`, Cost 700) so the per-step charge
/// machine actually moves credits — `empty_rules()` has no type, so cost resolves to 0
/// and the charge/cancel/stall paths are inert. The cost (700) divides to a rate of 12
/// and a clean per-step ladder. `BEAG` (Cost 600) gives a second category for the
/// same-tick two-Begin ordering test.
fn vehicle_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        // TechLevel=1 (not the unspecified -1 default) so the type passes Strict eligibility
        // and P6 prereq-revalidation does NOT abandon it as UnbuildableTechLevel; GAWEAP
        // (Factory=UnitType) is the producing factory the revalidation requires.
        "[VehicleTypes]\n0=GRIZZLY\n[AircraftTypes]\n0=BEAG\n\
         [BuildingTypes]\n0=GAWEAP\n\
         [GRIZZLY]\nCost=700\nTechLevel=1\n[BEAG]\nCost=600\nTechLevel=1\n\
         [GAWEAP]\nFactory=UnitType\n",
    ))
    .expect("vehicle rules parse")
}

/// Spawn a built-up War Factory (`GAWEAP`, `Factory=UnitType`) for `owner` so a Vehicle build
/// is genuinely buildable — P6 prereq-revalidation abandons an active build whose producing
/// factory is absent, so the charge-machinery guards (which drive `advance_tick`) need a real
/// factory present or the build is correctly abandoned before any charge.
fn spawn_war_factory(sim: &mut Simulation, owner: InternedId) {
    let gaweap = sim.interner.intern("GAWEAP");
    let owner_name = sim.interner.resolve(owner).to_string();
    let mut e = GameEntity::test_default(1, "GAWEAP", &owner_name, 5, 5);
    e.category = EntityCategory::Structure;
    e.owner = owner;
    e.type_ref = gaweap;
    e.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(e);
}

/// Rules with exactly one OrePurifier building type (`GAPROC`). Used by the
/// purifier-count guard — proves the base is the building COUNT (the v2 finding),
/// not silo storage capacity.
fn rules_with_ore_purifier() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n1=GAPROC\n[GAPROC]\nOrePurifier=yes\n",
    ))
    .expect("ore-purifier rules parse")
}

/// Arm a build directly on the FactoryRegistry (the P5d queue-of-record). Replaces the
/// retired `insert_queue(queued_item(..))` pattern: `enqueue` creates-or-re-arms the
/// active build for `(owner, cat)`, or appends a `QueueEntry` to the FIFO tail when an
/// active object is already held (a second `arm` call with a higher `order`). The cost is
/// resolved from `rules` (0 for `empty_rules`, matching the old shadow's zero-cost path).
fn arm(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    cat: ProductionCategory,
    ty: InternedId,
    total: u32,
    order: u64,
) {
    let cost = sim.object_type(ty, rules).map_or(0, |o| o.cost.max(0));
    sim.production
        .factory_shadow
        .test_enqueue_kernel(owner, cat, ty, order, total, cost);
}

/// Purifier refresh preserves the house's initialized cash balance.
#[test]
fn purifier_refresh_preserves_house_wallet() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let a = sim.interner.intern("Americans");
    let b = sim.interner.intern("Russians");
    sim.houses
        .insert(a, HouseState::new(a, 0, None, true, 5000, 10));
    sim.houses
        .insert(b, HouseState::new(b, 1, None, true, 1234, 10));
    sim.refresh_economy_shadow(Some(&rules));
    assert_eq!(sim.houses[&a].economy.credits, 5000);
    assert_eq!(sim.houses[&b].economy.credits, 1234);
    // The purifier-count statistic is still recomputed each refresh.
    assert_eq!(sim.houses[&a].economy.purifier_count, 0);
}

#[test]
fn economy_shadow_does_not_create_houses() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let before_len = sim.houses.len();
    let before = sim.state_hash();
    sim.refresh_economy_shadow(Some(&rules));
    assert_eq!(
        sim.houses.len(),
        before_len,
        "shadow must not create houses"
    );
    assert_eq!(before, sim.state_hash(), "shadow must not perturb the hash");
}

#[test]
fn economy_shadow_does_not_change_state_hash() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let a = sim.interner.intern("Americans");
    sim.houses
        .insert(a, HouseState::new(a, 0, None, true, 5000, 10));
    let before = sim.state_hash();
    sim.refresh_economy_shadow(Some(&rules));
    let after = sim.state_hash();
    assert_eq!(
        before, after,
        "economy shadow must not perturb the state hash"
    );
}

/// CONCERN-1 regression guard for the v2 correction: the purifier-bonus base is the
/// OrePurifier *building count* (NOT silo storage capacity). Would fail if the impl
/// ever modeled storage capacity (it counts owned OrePurifier structures).
#[test]
fn economy_purifier_count_is_building_count() {
    let mut sim = Simulation::new();
    let rules = rules_with_ore_purifier();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 5000, 10));
    let proc_ty = sim.interner.intern("GAPROC");
    let powr_ty = sim.interner.intern("GAPOWR"); // not a purifier (absent from rules)

    let add_structure = |sim: &mut Simulation, id: u64, ty: InternedId, rx: u16| {
        let mut e = GameEntity::test_default(id, "GAPROC", "Americans", rx, 5);
        e.category = EntityCategory::Structure;
        e.owner = owner;
        e.type_ref = ty;
        e.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(e);
    };
    // One purifier -> count 1.
    add_structure(&mut sim, 1, proc_ty, 5);
    sim.refresh_economy_shadow(Some(&rules));
    assert_eq!(sim.houses[&owner].economy.purifier_count, 1);

    // Second purifier + a non-purifier structure -> count 2 (the power plant is
    // NOT counted, proving it tracks OrePurifier buildings specifically).
    add_structure(&mut sim, 2, proc_ty, 6);
    add_structure(&mut sim, 3, powr_ty, 7);
    sim.refresh_economy_shadow(Some(&rules));
    assert_eq!(
        sim.houses[&owner].economy.purifier_count, 2,
        "purifier_count is the OrePurifier building count, not storage capacity"
    );
}

// ===== P2 — Factory / FactoryRegistry shadow =====

/// The registry is AUTHORITATIVE for progress, NOT derived from frames. The SEED arm
/// (`enqueue`) seeds a fresh build at progress 0; the PERSIST arm keeps the authoritative
/// progress across a `refresh_production_shadow` (now a no-op for the registry — there is
/// no reconcile to re-derive progress from frames).
#[test]
fn factory_reconcile_seeds_zero_and_persists_progress() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    let ty = sim.interner.intern("GRIZZLY");
    // Arm a fresh Vehicle build: the registry SEEDS progress 0 (authoritative), never
    // frames-derived.
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    {
        let view = sim
            .production
            .factory_shadow
            .view(owner, ProductionCategory::Vehicle)
            .expect("factory exists");
        assert_eq!(
            view.progress, 0,
            "SEED arm seeds a fresh build at progress 0, not frames-derived"
        );
        assert!(view.object.is_some(), "Building front => active object");
    }
    // Manually advance the authoritative progress, then refresh: the PERSIST arm must
    // leave progress UNTOUCHED (the registry persists with no rebuild).
    sim.production
        .factory_shadow
        .test_first_mut()
        .unwrap()
        .progress = 9;
    sim.refresh_production_shadow(Some(&rules));
    let view = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(
        view.progress, 9,
        "PERSIST arm keeps authoritative progress; frames are not the source"
    );
}

#[test]
fn factory_registry_iteration_is_insertion_ordered() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    // Distinct, monotonic enqueue_order per (owner, category) so the temporal mint
    // (insertion_seq = front.enqueue_order) yields distinct seqs — keeping the
    // monotonic-ordering assertion meaningful rather than vacuous all-equal.
    let mut order = 0u64;
    for (i, name) in ["A", "B", "C"].iter().enumerate() {
        let owner = sim.interner.intern(name);
        let ty = sim.interner.intern(&format!("U{i}"));
        for cat in [ProductionCategory::Vehicle, ProductionCategory::Infantry] {
            order += 1;
            arm(&mut sim, &rules, owner, cat, ty, 54, order);
        }
    }
    let seqs: Vec<u64> = sim
        .production
        .factory_shadow
        .iter_insertion_ordered()
        .iter()
        .map(|f| f.insertion_seq)
        .collect();
    let mut sorted = seqs.clone();
    sorted.sort();
    assert_eq!(seqs, sorted, "iteration is monotonic in insertion_seq");
    assert_eq!(seqs.len(), 6, "3 owners x 2 categories = 6 factories");
}

#[test]
fn insertion_seq_stable_across_rebuild() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    let seq_a = sim.production.factory_shadow.iter_insertion_ordered()[0].insertion_seq;
    // Advance the build, refresh — the registry persists, the same (owner, category)
    // survives with the same seq (refresh no longer reconciles).
    sim.production
        .factory_shadow
        .test_first_mut()
        .unwrap()
        .progress = 10;
    sim.refresh_production_shadow(Some(&rules));
    let seq_b = sim.production.factory_shadow.iter_insertion_ordered()[0].insertion_seq;
    assert_eq!(
        seq_a, seq_b,
        "surviving factory keeps a stable insertion_seq"
    );
}

/// The authority-flip inversion of `factory_registry_shadow_no_hash_change`: the
/// registry + economy statistics are now AUTHORITATIVE and hashed. Mutating each
/// newly-hashed field must move `state_hash()`.
#[test]
fn production_authoritative_hash_includes_factory_fields() {
    fn mid_build() -> Simulation {
        let mut sim = Simulation::new();
        let rules = vehicle_rules();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
        let ty = sim.interner.intern("GRIZZLY");
        arm(
            &mut sim,
            &rules,
            owner,
            ProductionCategory::Vehicle,
            ty,
            54,
            1,
        );
        sim
    }
    let base = mid_build().state_hash();

    type FMut = fn(&mut crate::sim::production::Factory);
    let factory_muts: [FMut; 9] = [
        |f| f.progress += 1,
        |f| f.balance += 1,
        |f| f.step_timer += 1,
        |f| f.on_hold = !f.on_hold,
        |f| f.suspended = !f.suspended,
        |f| f.original_balance += 1,
        |f| f.step_rate_frames += 1,
        |f| f.manual = !f.manual,
        |f| f.special = crate::sim::production::SpecialItem::NoneZero,
    ];
    for m in factory_muts {
        let mut sim = mid_build();
        m(sim.production.factory_shadow.test_first_mut().unwrap());
        assert_ne!(
            base,
            sim.state_hash(),
            "a newly-hashed Factory field must move the hash"
        );
    }

    type EMut = fn(&mut crate::sim::economy::Economy);
    let econ_muts: [EMut; 3] = [
        |e| e.spent_credits += 1,
        |e| e.harvested_credits += 1,
        |e| e.purifier_count += 1,
    ];
    for m in econ_muts {
        let mut sim = mid_build();
        let owner = sim.interner.intern("Americans");
        m(&mut sim.houses.get_mut(&owner).unwrap().economy);
        assert_ne!(
            base,
            sim.state_hash(),
            "a hashed economy statistic must move the hash"
        );
    }
}

#[test]
fn production_shadow_does_not_create_houses() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Ghost"); // no HouseState inserted
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    let before_houses = sim.houses.len();
    let before_factories = sim.production.factory_shadow.len();
    sim.refresh_production_shadow(Some(&rules));
    // The refresh NEVER fabricates a house (the auto-create hazard guard); the registry
    // (now authoritative + hashed) is populated by the arm, so the hash IS allowed to
    // move — only the no-house-creation invariant is asserted here.
    assert_eq!(
        sim.houses.len(),
        before_houses,
        "refresh must not create houses"
    );
    assert_eq!(
        sim.production.factory_shadow.len(),
        before_factories,
        "registry unchanged by the refresh no-op"
    );
}

/// FIT (a): the factory shell trace visits live Structures in LogicVector order.
/// The injected order [3, 1, 2] is NOT entity-id-sorted ([1, 2, 3]) — so an equal
/// assertion proves the trace follows LogicVector order, not BTreeMap/id order.
#[test]
fn factory_shadow_trace_order_matches_logic_vector() {
    let mut sim = Simulation::new();
    for id in [3u64, 1, 2] {
        let mut e = GameEntity::test_default(id, "GAPOWR", "Americans", 5, 5);
        e.category = EntityCategory::Structure;
        sim.substrate.entities.insert(e);
    }
    sim.set_logic_order_for_test(vec![3, 1, 2]);
    assert_eq!(
        sim.factory_shell_trace_order(),
        vec![3, 1, 2],
        "trace follows LogicVector order, not entity-id/map order"
    );
    sim.debug_assert_factory_shell_trace(); // intrinsic invariants must hold
}

/// The authority-flip inversion of `snapshot_roundtrip_ignores_shadow`: the registry
/// is now serialized + hashed, so a mid-build factory survives save->load
/// bit-identically AND the first post-load reconcile (PERSIST arm) leaves it untouched.
#[test]
fn snapshot_roundtrip_factory_registry() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    let next = sim.interner.intern("FV");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    ); // SEED arm: balance = full cost
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        next,
        54,
        2,
    ); // a tail entry to round-trip
    // Give the build non-trivial authoritative progress/balance/stats to round-trip.
    {
        let f = sim.production.factory_shadow.test_first_mut().unwrap();
        f.progress = 20;
        f.balance = 300;
        f.step_timer = 4;
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy
        .harvested_credits = 12_345;
    // Native in-scenario load resets Scenario RNG; isolate factory persistence
    // by comparing against that same post-load baseline.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let before = sim.state_hash();

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("load")
        .sim;
    assert_eq!(
        loaded.state_hash(),
        before,
        "registry + economy stats round-trip bit-identically"
    );

    // The first post-load reconcile (PERSIST arm, same front) must NOT perturb it.
    loaded.refresh_production_shadow(Some(&rules));
    assert_eq!(
        before,
        loaded.state_hash(),
        "post-load reconcile leaves the loaded build untouched"
    );
}

/// `progress_carry` was the retired frames-timer field with no live reader. P5d retired
/// the entire `BuildQueueItem`/`queues_by_owner` queue-of-record (along with
/// `progress_carry` and `remaining_base_frames`), so there is no longer a per-queue-item
/// hash fold for any of these fields to exercise. The original assertion ("mutating
/// progress_carry leaves the hash unchanged") can no longer be expressed — the field does
/// not exist. The equivalent invariant now: the registry has no per-queue-item retired
/// frames field to fold, so the only hashed production state is the `Factory` head fields
/// + `QueueEntry` (type/order/total) — none of which is a `progress_carry`/`remaining`
/// frames timer.
// P5D-REVIEW: original tested mutation of the RETIRED BuildQueueItem.progress_carry field;
// that field no longer exists. Re-expressed as "no retired frames-timer field is hashed".
// Human: confirm this is the intended equivalent, or delete the test.
#[test]
#[ignore = "P5D-REVIEW: BuildQueueItem.progress_carry retired; original mutation no longer expressible"]
fn legacy_progress_carry_removed_from_hash() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    let before = sim.state_hash();
    // No retired per-queue-item frames field exists to mutate; a refresh no-op must not
    // perturb the hash (the registry carries no progress_carry/remaining frames timer).
    sim.refresh_production_shadow(Some(&rules));
    assert_eq!(
        before,
        sim.state_hash(),
        "no retired progress_carry frames field is hashed"
    );
}

/// Identical fixtures over N ticks produce identical per-tick state_hash sequences
/// (the production shadow keeps advance_tick deterministic).
#[test]
fn production_shadow_preserves_advance_tick_phase_order() {
    fn run() -> Vec<u64> {
        let mut sim = Simulation::new();
        let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        (0..5)
            .map(|_| {
                sim.advance_tick(&[], None, &heights, None, None, 67);
                sim.state_hash()
            })
            .collect()
    }
    assert_eq!(
        run(),
        run(),
        "advance_tick with the production shadow stays deterministic"
    );
}

// ===== P3 — per-step charge oracle (hash-neutral) =====

/// P3 no-hash guarantee: stepping a CLONE of a shadow factory against a CLONE of the
/// wallet 54 times leaves `state_hash()` bit-identical (the oracle never touches the
/// hashed wallet; `Factory`/`Economy` carry no serde derive). The acceptance test.
#[test]
fn factory_advance_step_does_not_change_state_hash() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    ); // cost-based shadow built
    let before = sim.state_hash();

    // Step a CLONE of the shadow factory against a CLONE of the wallet, 54 times.
    let mut f = sim.production.factory_shadow.iter_insertion_ordered()[0].clone();
    // empty_rules -> cost 0; seed a real cost so the step machine actually charges.
    f.progress = 0;
    f.balance = 700;
    f.original_balance = 700;
    let mut oracle = sim.houses[&owner].economy.clone();
    for _ in 0..PRODUCTION_STEPS {
        let _ = f.advance_one_step(&mut oracle);
    }

    assert_eq!(
        before,
        sim.state_hash(),
        "P3 oracle stepping must not perturb the state hash (serde-skip + clone)"
    );
    assert_eq!(
        sim.houses[&owner].economy.credits, 1_000_000,
        "the legacy wallet is untouched by oracle stepping"
    );
}

/// P3 determinism: identical fixtures over N ticks (with the cost-based rebuild +
/// the conservation assert active in debug) produce identical per-tick state_hash
/// sequences. `rules` is the 2nd positional arg to `advance_tick`.
#[test]
fn production_shadow_with_oracle_is_deterministic() {
    fn run() -> Vec<u64> {
        let mut sim = Simulation::new();
        let rules = empty_rules();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
        let ty = sim.interner.intern("GRIZZLY");
        arm(
            &mut sim,
            &rules,
            owner,
            ProductionCategory::Vehicle,
            ty,
            54,
            1,
        );
        let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        (0..5)
            .map(|_| {
                sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
                sim.state_hash()
            })
            .collect()
    }
    assert_eq!(
        run(),
        run(),
        "advance_tick with the P3 oracle path stays deterministic"
    );
}

/// The FIT-(a) probe: build a cost-based shadow, place a live Structure for the
/// owner, and confirm the oracle probe steps a clone per (live structure, owner
/// factory-with-object) — read-only, deterministic, hash-neutral.
#[test]
fn factory_oracle_step_trace_walks_live_structures() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    // A live Structure for the owner (the war factory the probe walks).
    let mut e = GameEntity::test_default(1, "GAWEAP", "Americans", 5, 5);
    e.category = EntityCategory::Structure;
    e.owner = owner;
    sim.substrate.entities.insert(e);
    sim.set_logic_order_for_test(vec![1]);
    sim.refresh_production_shadow(Some(&rules));

    let before = sim.state_hash();
    let trace = sim.factory_oracle_step_trace();
    assert_eq!(
        trace.len(),
        1,
        "one live structure x one owner factory-with-object"
    );
    assert_eq!(
        trace[0].0, 1,
        "the outcome is attributed to the live structure id"
    );
    assert_eq!(
        before,
        sim.state_hash(),
        "the probe must not perturb the hash"
    );
    assert_eq!(
        trace,
        sim.factory_oracle_step_trace(),
        "the probe is deterministic across calls"
    );
}

// ===== P4 — FIFO queue + cancel + partial refund (hash-neutral oracle) =====

/// P4 no-hash guarantee (mirrors `factory_advance_step_does_not_change_state_hash`):
/// cancelling a mid-build active object on a CLONE of the registry against a CLONE of
/// the wallet leaves `state_hash()` bit-identical. With `empty_rules` the cost is 0
/// (refund 0); the contract here is the HASH + legacy-wallet invariants, which hold
/// regardless of the refund value (the nonzero refund is proven in the pure
/// `cancel_one_active_when_no_queued_copy` test).
#[test]
fn factory_cancel_one_does_not_change_state_hash() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    ); // cost-based shadow built
    let before = sim.state_hash();
    let legacy_credits = sim.houses[&owner].economy.credits;

    // Cancel (active abandon, mid-build) against a CLONE of the registry + a CLONE of
    // the wallet; the active GRIZZLY has no queued copy, so the active-abandon branch
    // fires (AbandonedActive).
    let mut reg = sim.production.factory_shadow.clone();
    let mut oracle = sim.houses[&owner].economy.clone();
    let outcome = reg.test_cancel_one_kernel(owner, ProductionCategory::Vehicle, ty, &mut oracle);
    assert!(
        matches!(outcome, CancelOutcome::AbandonedActive { .. }),
        "the active build (no queued copy) is abandoned on the clone"
    );

    assert_eq!(
        before,
        sim.state_hash(),
        "P4 cancel on a clone must not perturb the state hash (serde-skip + clone)"
    );
    assert_eq!(
        sim.houses[&owner].economy.credits, legacy_credits,
        "the legacy wallet is untouched by the oracle cancel"
    );
}

/// P4 C7/C12: completion suspends with the object attached; `start_next_queued` does
/// NOT advance while the object is held; only after the object is CLEARED (simulating
/// the delivery commit, a later slice) does the queue front advance. Proves the
/// negative invariant end-to-end WITHOUT wiring delivery. Driven on a CLONE.
#[test]
fn queue_advances_only_after_delivery() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let active = sim.interner.intern("GRIZZLY");
    let next = sim.interner.intern("FV"); // the queued tail item
    // Front Building (active object) with a tail item behind it.
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        active,
        54,
        1,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        next,
        54,
        2,
    );

    let before = sim.state_hash();
    let mut f = sim.production.factory_shadow.iter_insertion_ordered()[0].clone();
    assert_eq!(
        f.object.as_ref().map(|o| o.type_id),
        Some(active),
        "active = GRIZZLY"
    );
    assert_eq!(
        f.queue.iter().map(|e| e.type_id).collect::<Vec<_>>(),
        vec![next],
        "tail = [FV]"
    );
    // empty_rules -> cost 0; seed a real cost so completion takes the full ladder.
    f.progress = 0;
    f.balance = 700;
    f.original_balance = 700;
    // The credits mirror is retired, so a cloned economy starts at 0 — fund the oracle
    // explicitly so the per-step charge can actually complete the build.
    let mut oracle = sim.houses[&owner].economy.clone();
    oracle.credits = 700;
    loop {
        if matches!(f.advance_one_step(&mut oracle), StepOutcome::Completed) {
            break;
        }
    }
    assert!(
        f.suspended && f.object.is_some(),
        "C12: completion holds the object, suspended"
    );
    // The queue does NOT advance on completion alone (cost/step_delay are inert when the
    // object is still held — the guard fires before the seed).
    assert_eq!(
        f.start_next_queued(0, 0),
        None,
        "C7: held object blocks the advance"
    );
    assert_eq!(
        f.queue.iter().map(|e| e.type_id).collect::<Vec<_>>(),
        vec![next],
        "queue front unchanged while the object is held"
    );
    // Simulate the delivery commit: clear the object, THEN the queue advances (delivery
    // path: step_delay 0).
    f.object = None;
    f.suspended = false;
    assert_eq!(
        f.start_next_queued(0, 0),
        Some(next),
        "after delivery the front pops"
    );
    assert_eq!(
        f.object.as_ref().map(|o| o.type_id),
        Some(next),
        "active = FV"
    );
    assert!(f.queue.is_empty(), "tail consumed");

    assert_eq!(
        before,
        sim.state_hash(),
        "the clone drive must not perturb the hash"
    );
}

/// P4 determinism: identical fixtures over N ticks with a per-tick cancel probe on
/// CLONES produce identical per-tick state_hash sequences (mirrors
/// `production_shadow_with_oracle_is_deterministic`).
#[test]
fn production_shadow_with_cancel_is_deterministic() {
    fn run() -> Vec<u64> {
        let mut sim = Simulation::new();
        let rules = empty_rules();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
        let ty = sim.interner.intern("GRIZZLY");
        arm(
            &mut sim,
            &rules,
            owner,
            ProductionCategory::Vehicle,
            ty,
            54,
            1,
        );
        let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        (0..5)
            .map(|_| {
                sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
                // Per-tick clone cancel probe (NEVER written back).
                let mut reg = sim.production.factory_shadow.clone();
                let mut oracle = sim
                    .houses
                    .get(&owner)
                    .map(|h| h.economy.clone())
                    .unwrap_or_default();
                let _ =
                    reg.test_cancel_one_kernel(owner, ProductionCategory::Vehicle, ty, &mut oracle);
                sim.state_hash()
            })
            .collect()
    }
    assert_eq!(
        run(),
        run(),
        "advance_tick with the P4 clone cancel probe stays deterministic"
    );
}

// ===== P5a — flip-prep (pure producers + temporal mint + inversion-readiness, hash-neutral) =====

/// P5a no-hash guarantee (the acceptance test; mirrors
/// `factory_advance_step_does_not_change_state_hash` /
/// `factory_cancel_one_does_not_change_state_hash`): building the producer, stepping a
/// CLONE factory against a CLONE economy, and running the dormant delivery probe leaves
/// `state_hash()` bit-identical (no serde derive; no authoritative call site; the mint
/// change touches only the `#[serde(skip)]` registry).
#[test]
fn factory_flip_prep_does_not_change_state_hash() {
    use crate::sim::production::{BuildStepTimeInputs, build_step_time};
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    let before = sim.state_hash();
    let legacy_credits = sim.houses[&owner].economy.credits;

    // Run every P5a piece against CLONES / pure values.
    let total = build_step_time(&BuildStepTimeInputs {
        cost: 700,
        build_time_bonus_ppm: 1_000_000,
        build_time_multiplier_ppm: 1_000_000,
        power_ratio_ppm: 1_000_000,
        low_power_penalty_modifier_ppm: 1_000_000,
        min_clamp_ppm: 500_000,
        max_clamp_ppm: 900_000,
        multiple_factory_ppm: 800_000,
        factory_count: 1,
        is_wall: false,
        wall_build_speed_ppm: 1_000_000,
    });
    assert_eq!(total, 700, "producer is pure, returns the TOTAL");
    let mut f = sim.production.factory_shadow.iter_insertion_ordered()[0].clone();
    f.set_rate(total);
    let mut oracle = sim.houses[&owner].economy.clone();
    for _ in 0..PRODUCTION_STEPS {
        let _ = f.advance_one_step(&mut oracle);
    }
    let _probe = sim.factory_delivery_probe(); // dormant; clone-only

    assert_eq!(
        before,
        sim.state_hash(),
        "P5a flip-prep on clones/pure values must not perturb the state hash"
    );
    assert_eq!(
        sim.houses[&owner].economy.credits, legacy_credits,
        "the legacy wallet is untouched by the flip-prep"
    );
}

/// P5a Lane-A mint: after `refresh_production_shadow`, each factory's `insertion_seq`
/// equals its queue front's `enqueue_order` (the temporal first-Begin stamp), NOT the
/// old BTreeMap sorted-(owner, category) mint. Aircraft begun BEFORE Vehicle (lower
/// enqueue_order) must sweep first even though Vehicle sorts before Aircraft by enum.
#[test]
fn factory_insertion_seq_equals_front_enqueue_order() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    let air_ty = sim.interner.intern("BEAG");
    let veh_ty = sim.interner.intern("GRIZZLY");
    // Aircraft begun first (order 10), Vehicle second (order 20) — enqueue mints each
    // factory's insertion_seq from its first-begin order.
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Aircraft,
        air_ty,
        54,
        10,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        veh_ty,
        54,
        20,
    );

    let ordered: Vec<(ProductionCategory, u64)> = sim
        .production
        .factory_shadow
        .iter_insertion_ordered()
        .iter()
        .map(|f| (f.category, f.insertion_seq))
        .collect();
    assert_eq!(
        ordered,
        vec![
            (ProductionCategory::Aircraft, 10),
            (ProductionCategory::Vehicle, 20)
        ],
        "insertion_seq == front.enqueue_order; sweep follows TEMPORAL, not enum-sort, order"
    );
}

/// P5a Lane-A order: the sweep visits Aircraft (begun first) before Vehicle (begun
/// second) — the DRIFT-fix vs the old sorted mint, exposed as a positive blocking test.
#[test]
fn factory_step_order_matches_legacy_temporal_order() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    let air_ty = sim.interner.intern("BEAG");
    let veh_ty = sim.interner.intern("GRIZZLY");
    // Aircraft begun first (order 5), Vehicle second (order 9).
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Aircraft,
        air_ty,
        54,
        5,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        veh_ty,
        54,
        9,
    );

    let cats_in_sweep: Vec<ProductionCategory> = sim
        .production
        .factory_shadow
        .iter_insertion_ordered()
        .iter()
        .map(|f| f.category)
        .collect();
    assert_eq!(
        cats_in_sweep,
        vec![ProductionCategory::Aircraft, ProductionCategory::Vehicle],
        "sweep visits the earlier-begun Aircraft first (temporal), not Vehicle (enum-sort)"
    );
}

/// P5a inversion-readiness: drive `advance_tick` over N ticks; the live debug assert
/// `debug_assert_factory_step_matches_legacy` runs each tick and a clean run (no panic)
/// proves the (A) order + (D) delivery invariants hold across the suite.
#[test]
fn factory_step_matches_legacy_shadow_holds() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    for _ in 0..5 {
        // If the inversion assert diverges, advance_tick panics in a debug build.
        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
    }
}

/// P5a delivery seam is DORMANT: the probe is test-only and reports the post-delivery
/// pop on a CLONE; the live shadow front is unchanged (no advance_tick path invokes
/// start_next_queued) and the hash is untouched.
#[test]
fn production_delivery_probe_is_dormant() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let active = sim.interner.intern("GRIZZLY");
    let next = sim.interner.intern("FV");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        active,
        54,
        1,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        next,
        54,
        2,
    );

    let before = sim.state_hash();
    let probe = sim.factory_delivery_probe();
    assert_eq!(probe.len(), 1, "one factory with a tail");
    assert_eq!(
        probe[0].2,
        Some(next),
        "the probe would pop FV after a delivery (on a clone)"
    );
    let view = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Vehicle)
        .unwrap();
    assert_eq!(
        view.object.map(|o| o.type_id),
        Some(active),
        "live active still GRIZZLY"
    );
    assert_eq!(
        view.queue.iter().map(|e| e.type_id).collect::<Vec<_>>(),
        vec![next],
        "live tail unchanged"
    );
    assert_eq!(
        before,
        sim.state_hash(),
        "the probe must not perturb the hash"
    );
}

/// P5a determinism: a per-tick closure that builds the producer + runs the dormant
/// probe on clones produces identical per-tick state_hash sequences across two runs
/// (mirrors `production_shadow_with_cancel_is_deterministic`).
#[test]
fn production_flip_prep_is_deterministic() {
    use crate::sim::production::{BuildStepTimeInputs, build_step_time};
    fn run() -> Vec<u64> {
        let mut sim = Simulation::new();
        let rules = empty_rules();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
        let ty = sim.interner.intern("GRIZZLY");
        arm(
            &mut sim,
            &rules,
            owner,
            ProductionCategory::Vehicle,
            ty,
            54,
            1,
        );
        let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        (0..5)
            .map(|_| {
                sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
                // Per-tick flip-prep probe on clones / pure values (NEVER written back).
                let _ = build_step_time(&BuildStepTimeInputs {
                    cost: 700,
                    build_time_bonus_ppm: 1_000_000,
                    build_time_multiplier_ppm: 1_000_000,
                    power_ratio_ppm: 1_000_000,
                    low_power_penalty_modifier_ppm: 1_000_000,
                    min_clamp_ppm: 500_000,
                    max_clamp_ppm: 900_000,
                    multiple_factory_ppm: 800_000,
                    factory_count: 1,
                    is_wall: false,
                    wall_build_speed_ppm: 1_000_000,
                });
                let _ = sim.factory_delivery_probe();
                sim.state_hash()
            })
            .collect()
    }
    assert_eq!(
        run(),
        run(),
        "advance_tick with the P5a flip-prep probe stays deterministic"
    );
}

// ===== P5b — the authority flip: real-wallet charge guards (end-to-end via advance_tick) =====

/// §3.3/C15: over a full build the per-step charge (`step_all`, wired at the Phase-7
/// head) debits EXACTLY the full cost ONCE from the one wallet (`house.economy.credits`), and
/// `economy.spent_credits` accumulates the same. The end-to-end proof of the charge flip
/// (no upfront debit, no double-charge) — drives `advance_tick`, not a clone.
#[test]
fn single_wallet_charged_once_no_double_debit() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    spawn_war_factory(&mut sim, owner); // P6: a real factory so the build is not abandoned
    let full_cost = sim
        .object_type(ty, &rules)
        .map(|o| o.cost.max(0))
        .unwrap_or(0);
    assert!(
        full_cost > 0,
        "GRIZZLY needs a positive cost for this guard"
    );
    let start = sim.houses[&owner].economy.credits;
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    // A war factory exists but no path_grid is supplied, so the completed vehicle has no exit
    // cell and is held (delivery never fires) — the build charges to completion exactly once
    // and never re-seeds. Upper-bound the cadence (<= 255 frames/step * 54 steps) and break
    // once the cost is fully drained.
    for _ in 0..(PRODUCTION_STEPS as usize * 256) {
        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
        if sim.houses[&owner].economy.spent_credits >= full_cost {
            break;
        }
    }
    let debited = start - sim.houses[&owner].economy.credits;
    assert_eq!(
        debited, full_cost,
        "exactly one full-cost debit to house.economy.credits over the build"
    );
    assert_eq!(
        sim.houses[&owner].economy.spent_credits, full_cost,
        "spent_credits accumulates the cost exactly once"
    );
}

/// C4: a 0-credit house cannot afford the per-step charge -> the factory stalls
/// (`on_hold`), spending NOTHING against the real wallet (the strict-< stall, end-to-end).
#[test]
fn stall_on_no_funds_holds() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 0, 10)); // 0 credits
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    spawn_war_factory(&mut sim, owner); // P6: factory present so the build STALLS (not abandoned)
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    for _ in 0..200 {
        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
    }
    assert_eq!(
        sim.houses[&owner].economy.credits, 0,
        "a stalled build spends nothing"
    );
    assert_eq!(
        sim.houses[&owner].economy.spent_credits, 0,
        "nothing is accumulated while stalled"
    );
}

/// C8: cancelling a mid-build active object refunds EXACTLY the spent portion
/// (`original_balance - balance`) into the one wallet (`house.economy.credits`), NOT the full
/// cost (the legacy `.rev()` full refund is the retired DRIFT). Drives a real charge.
#[test]
fn cancel_one_partial_refund_to_house_credits() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        ty,
        54,
        1,
    );
    spawn_war_factory(&mut sim, owner); // P6: factory present so the build is not abandoned
    let full_cost = sim
        .object_type(ty, &rules)
        .map(|o| o.cost.max(0))
        .unwrap_or(0);
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    // Charge partway (not to completion).
    for _ in 0..200 {
        sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
    }
    let spent = sim.houses[&owner].economy.spent_credits;
    assert!(
        spent > 0 && spent < full_cost,
        "mid-build: some but not all of the cost is spent"
    );
    let credits_before = sim.houses[&owner].economy.credits;
    let ok =
        crate::sim::production::cancel_by_type_for_owner(&mut sim, &rules, "Americans", "GRIZZLY");
    assert!(ok, "the active build is cancellable");
    let refunded = sim.houses[&owner].economy.credits - credits_before;
    assert_eq!(
        refunded, spent,
        "C8: refund exactly the spent portion (original_balance - balance)"
    );
    // The cancelled active build (no tail) leaves an idle factory that is pruned, so the
    // factory no longer exists in the registry (the queue-of-record).
    assert!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Vehicle)
            .is_none(),
        "the cancelled active build left the queue-of-record"
    );
}

/// Lockstep determinism across the bump: two sims run the SAME scripted command stream
/// (two owners, two categories with distinct enqueue_orders, a same-tick two-Begin, and a
/// mid-stream cancel) and MUST produce an identical per-tick `state_hash` sequence. The
/// flip's near-term lockstep guard (the global replay/parity gate is the later P5c slice).
#[test]
fn factory_flip_determinism_over_scripted_commands() {
    fn run() -> Vec<u64> {
        let mut sim = Simulation::new();
        let rules = vehicle_rules();
        let a = sim.interner.intern("Americans");
        let b = sim.interner.intern("Russians");
        sim.houses
            .insert(a, HouseState::new(a, 0, None, true, 1_000_000, 10));
        sim.houses
            .insert(b, HouseState::new(b, 1, None, true, 1_000_000, 10));
        let griz = sim.interner.intern("GRIZZLY");
        let beag = sim.interner.intern("BEAG");
        // Owner A: a Vehicle build (order 1) + an Aircraft build (order 2).
        arm(
            &mut sim,
            &rules,
            a,
            ProductionCategory::Vehicle,
            griz,
            54,
            1,
        );
        arm(
            &mut sim,
            &rules,
            a,
            ProductionCategory::Aircraft,
            beag,
            54,
            2,
        );
        // Owner B: a Vehicle build (order 3).
        arm(
            &mut sim,
            &rules,
            b,
            ProductionCategory::Vehicle,
            griz,
            54,
            3,
        );
        sim.production.next_enqueue_order = 4;

        let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        (0..160)
            .map(|i| {
                if i == 10 {
                    // Cancel one of A's builds (the Aircraft) partway through.
                    let _ = crate::sim::production::cancel_by_type_for_owner(
                        &mut sim,
                        &rules,
                        "Americans",
                        "BEAG",
                    );
                }
                sim.advance_tick(&[], Some(&rules), &heights, None, None, 67);
                sim.state_hash()
            })
            .collect()
    }
    assert_eq!(
        run(),
        run(),
        "the authority flip preserves lockstep determinism across the bump"
    );
}
