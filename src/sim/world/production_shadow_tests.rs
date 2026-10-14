//! Factory registry (`ProductionState.factories`) tests from the `world` level,
//! where `Simulation::new`, `advance_tick`, `state_hash`, `set_logic_order_for_test`
//! and the snapshot API are reachable: sweep order, the state-hash fold, snapshot
//! round trips, the per-step charge against the house wallet, cancel refunds and
//! the delivery-gated queue advance.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{PRODUCTION_STEPS, ProductionCategory, StepOutcome};
use crate::sim::timer::CdTimer;

fn empty_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str("")).expect("empty rules parse")
}

/// Rules with a costed buildable vehicle (`GRIZZLY`, Cost 700) so the per-step charge
/// machine actually moves credits — `empty_rules()` has no type, so cost resolves to 0
/// and the charge/cancel/stall paths are inert. `BEAG` (Cost 600) gives a second category for the
/// same-tick two-Begin ordering test.
fn vehicle_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        // TechLevel=1 (not the unspecified 255 default) so the strip keeps the type;
        // GAWEAP (Factory=UnitType) is the factory FindFactory finds for it, matching
        // its Owner=.
        "[Countries]\n0=Americans\n\
         [VehicleTypes]\n0=GRIZZLY\n[AircraftTypes]\n0=BEAG\n\
         [BuildingTypes]\n0=GAWEAP\n\
         [GRIZZLY]\nCost=700\nStrength=300\nTechLevel=1\nOwner=Americans\n\
         [BEAG]\nCost=600\nStrength=200\nTechLevel=1\nOwner=Americans\n\
         [GAWEAP]\nStrength=1000\nFactory=UnitType\nOwner=Americans\n",
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
    let mut e = GameEntity::test_default_of_category(
        1,
        "GAWEAP",
        &owner_name,
        5,
        5,
        EntityCategory::Structure,
    );
    e.set_owner_for_test(owner);
    e.set_type_ref_for_test(gaweap);
    e.lifecycle.set_in_limbo_for_test(false);
    e.in_playfield = true;
    e.finish_building_construction_for_test();
    e.building_art.set_actually_placed_for_test(true);
    sim.substrate.entities.insert(e);
    sim.add_entity_occupancy(1);
    sim.append_house_base_building_for_test(1);
    sim.substrate.next_stable_object_id = 2;
    sim.session.house_order.push(owner);
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
    order: u64,
) {
    let cost = sim.object_type(ty, rules).map_or(0, |o| o.cost.max(0));
    sim.production
        .factories
        .test_enqueue_kernel(owner, cat, ty, order, cost);
}

// ===== P2 — Factory / FactoryRegistry shadow =====

/// The registry is AUTHORITATIVE for progress, NOT derived from frames: the SEED arm
/// (`enqueue`) seeds a fresh build at progress 0.
#[test]
fn factory_enqueue_seeds_zero_progress() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    let ty = sim.interner.intern("GRIZZLY");
    // Arm a fresh Vehicle build: the registry SEEDS progress 0 (authoritative), never
    // frames-derived.
    arm(&mut sim, &rules, owner, ProductionCategory::Vehicle, ty, 1);
    {
        let view = sim
            .production
            .factories
            .view(owner, ProductionCategory::Vehicle)
            .expect("factory exists");
        assert_eq!(
            view.progress, 0,
            "SEED arm seeds a fresh build at progress 0, not frames-derived"
        );
        assert!(view.object.is_some(), "Building front => active object");
    }
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
            arm(&mut sim, &rules, owner, cat, ty, order);
        }
    }
    let seqs: Vec<u64> = sim
        .production
        .factories
        .iter_insertion_ordered()
        .iter()
        .map(|f| f.insertion_seq)
        .collect();
    let mut sorted = seqs.clone();
    sorted.sort();
    assert_eq!(seqs, sorted, "iteration is monotonic in insertion_seq");
    assert_eq!(seqs.len(), 6, "3 owners x 2 categories = 6 factories");
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
        arm(&mut sim, &rules, owner, ProductionCategory::Vehicle, ty, 1);
        sim
    }
    let base = mid_build().state_hash();

    type FMut = fn(&mut crate::sim::production::Factory);
    let factory_muts: [FMut; 8] = [
        |f| f.progress += 1,
        |f| f.balance += 1,
        |f| f.step_timer = CdTimer::started(123, f.step_timer.duration()),
        |f| f.step_timer = CdTimer::from_raw(f.step_timer.start_frame(), 5),
        |f| f.on_hold = !f.on_hold,
        |f| f.suspended = !f.suspended,
        |f| f.step_rate_frames += 1,
        |f| f.manual = !f.manual,
    ];
    for m in factory_muts {
        let mut sim = mid_build();
        m(sim.production.factories.test_first_mut().unwrap());
        assert_ne!(
            base,
            sim.state_hash(),
            "a newly-hashed Factory field must move the hash"
        );
    }

    type EMut = fn(&mut crate::sim::economy::Economy);
    let econ_muts: [EMut; 2] = [
        |e| e.set_spent_for_test(e.spent_credits() + 1),
        |e| e.set_score_for_test(e.score() + 1),
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

/// The authority-flip inversion of `snapshot_roundtrip_ignores_shadow`: the registry
/// is now serialized + hashed, so a mid-build factory survives save->load
/// bit-identically.
#[test]
fn snapshot_roundtrip_factory_registry() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    let next = sim.interner.intern("FV");
    arm(&mut sim, &rules, owner, ProductionCategory::Vehicle, ty, 1); // SEED arm: balance = full cost
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        next,
        2,
    ); // a tail entry to round-trip
    // Give the build non-trivial authoritative progress/balance/stats to round-trip.
    {
        let f = sim.production.factories.test_first_mut().unwrap();
        f.progress = 20;
        f.balance = 300;
        f.step_timer = CdTimer::started(90, 12);
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy
        .set_score_for_test(12_345);
    // Native in-scenario load resets Scenario RNG; isolate factory persistence
    // by comparing against that same post-load baseline.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let before = sim.state_hash();

    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("load")
        .sim;
    assert_eq!(
        loaded.state_hash(),
        before,
        "registry + economy stats round-trip bit-identically"
    );
}

/// Identical fixtures over N ticks produce identical per-tick state_hash sequences
/// (the production shadow keeps advance_tick deterministic).
#[test]
fn production_shadow_preserves_advance_tick_phase_order() {
    fn run() -> Vec<u64> {
        let mut sim = Simulation::new();
        (0..5)
            .map(|_| {
                sim.advance_tick(&[], None, None, None, 67);
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

// ===== P4 — FIFO queue =====

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
        1,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        next,
        2,
    );

    let before = sim.state_hash();
    let mut f = sim.production.factories.iter_insertion_ordered()[0].clone();
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
    // The credits mirror is retired, so a cloned economy starts at 0 — fund the oracle
    // explicitly so the per-step charge can actually complete the build.
    let mut oracle = sim.houses[&owner].economy.clone();
    oracle.set_credits_for_test(700);
    loop {
        if matches!(f.advance_one_step(&mut oracle), StepOutcome::Completed) {
            break;
        }
    }
    assert!(
        f.suspended && f.object.is_some(),
        "C12: completion holds the object, suspended"
    );
    // The queue does NOT advance on completion alone (the cost is inert when the
    // object is still held — the guard fires before the seed).
    assert_eq!(
        f.start_next_queued(0),
        None,
        "C7: held object blocks the advance"
    );
    assert_eq!(
        f.queue.iter().map(|e| e.type_id).collect::<Vec<_>>(),
        vec![next],
        "queue front unchanged while the object is held"
    );
    // Simulate the delivery commit: clear the object, THEN the queue advances.
    f.object = None;
    f.suspended = false;
    assert_eq!(
        f.start_next_queued(0),
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

// ===== P5a — flip-prep (pure producers + temporal mint + inversion-readiness, hash-neutral) =====

/// P5a Lane-A mint: each factory's `insertion_seq`
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
        10,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        veh_ty,
        20,
    );

    let ordered: Vec<(ProductionCategory, u64)> = sim
        .production
        .factories
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
        5,
    );
    arm(
        &mut sim,
        &rules,
        owner,
        ProductionCategory::Vehicle,
        veh_ty,
        9,
    );

    let cats_in_sweep: Vec<ProductionCategory> = sim
        .production
        .factories
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

/// Drive `advance_tick` over N ticks; `debug_assert_factory_invariants` runs each
/// tick in debug builds, and a clean run (no panic) shows its order and state
/// invariants hold.
#[test]
fn factory_step_matches_legacy_shadow_holds() {
    let mut sim = Simulation::new();
    let rules = empty_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    arm(&mut sim, &rules, owner, ProductionCategory::Vehicle, ty, 1);
    for _ in 0..5 {
        // A broken invariant panics inside advance_tick in a debug build.
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
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
    spawn_war_factory(&mut sim, owner); // P6: a real factory so the build is not abandoned
    // A live build needs Begin_Production's object constructor and timer start;
    // the queue-only `arm` fixture deliberately supplies neither.
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GRIZZLY",
        crate::sim::world::FrameEffects::default()
    ));
    let full_cost = sim
        .object_type(ty, &rules)
        .map(|o| o.cost.max(0))
        .unwrap_or(0);
    assert!(
        full_cost > 0,
        "GRIZZLY needs a positive cost for this guard"
    );
    let start = sim.houses[&owner].economy.credits();
    // A war factory exists but no path_grid is supplied, so the completed vehicle has no exit
    // cell and is held (delivery never fires) — the build charges to completion exactly once
    // and never re-seeds. Upper-bound the cadence (<= 255 frames/step * 54 steps) and break
    // once the cost is fully drained.
    for _ in 0..(PRODUCTION_STEPS as usize * 256) {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        if sim.houses[&owner].economy.spent_credits() >= full_cost {
            break;
        }
    }
    let debited = start - sim.houses[&owner].economy.credits();
    assert_eq!(
        debited, full_cost,
        "exactly one full-cost debit to house.economy.credits over the build"
    );
    assert_eq!(
        sim.houses[&owner].economy.spent_credits(),
        full_cost,
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
    spawn_war_factory(&mut sim, owner); // P6: factory present so the build STALLS (not abandoned)
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GRIZZLY",
        crate::sim::world::FrameEffects::default()
    ));
    for _ in 0..200 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert_eq!(
        sim.houses[&owner].economy.credits(),
        0,
        "a stalled build spends nothing"
    );
    assert_eq!(
        sim.houses[&owner].economy.spent_credits(),
        0,
        "nothing is accumulated while stalled"
    );
}

/// C8: cancelling a mid-build active object refunds its Cost_Of less the Balance it still
/// owed into the one wallet (`house.economy.credits`): with an unchanged Cost_Of, exactly
/// the spent portion, NOT the full cost. Drives a real charge.
#[test]
fn cancel_one_partial_refund_to_house_credits() {
    let mut sim = Simulation::new();
    let rules = vehicle_rules();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, true, 1_000_000, 10));
    let ty = sim.interner.intern("GRIZZLY");
    spawn_war_factory(&mut sim, owner); // P6: factory present so the build is not abandoned
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "GRIZZLY",
        crate::sim::world::FrameEffects::default()
    ));
    let full_cost = sim
        .object_type(ty, &rules)
        .map(|o| o.cost.max(0))
        .unwrap_or(0);
    // Charge partway (not to completion).
    for _ in 0..200 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    let spent = sim.houses[&owner].economy.spent_credits();
    assert!(
        spent > 0 && spent < full_cost,
        "mid-build: some but not all of the cost is spent"
    );
    let credits_before = sim.houses[&owner].economy.credits();
    let ok = crate::sim::production::cancel_by_type_for_owner(
        &mut sim,
        &rules,
        "Americans",
        "GRIZZLY",
        false,
        crate::sim::world::FrameEffects::default(),
    );
    assert!(ok, "the active build is cancellable");
    let refunded = sim.houses[&owner].economy.credits() - credits_before;
    assert_eq!(
        refunded, spent,
        "C8: Cost_Of - Balance refunds exactly the spent portion"
    );
    // The cancelled active build (no tail) leaves an idle factory that is pruned, so the
    // factory no longer exists in the registry (the queue-of-record).
    assert!(
        sim.production
            .factories
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
        arm(&mut sim, &rules, a, ProductionCategory::Vehicle, griz, 1);
        arm(&mut sim, &rules, a, ProductionCategory::Aircraft, beag, 2);
        // Owner B: a Vehicle build (order 3).
        arm(&mut sim, &rules, b, ProductionCategory::Vehicle, griz, 3);
        sim.production.next_enqueue_order = 4;

        (0..160)
            .map(|i| {
                if i == 10 {
                    // Cancel one of A's builds (the Aircraft) partway through.
                    let _ = crate::sim::production::cancel_by_type_for_owner(
                        &mut sim,
                        &rules,
                        "Americans",
                        "BEAG",
                        false,
                        crate::sim::world::FrameEffects::default(),
                    );
                }
                sim.advance_tick(&[], Some(&rules), None, None, 67);
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
