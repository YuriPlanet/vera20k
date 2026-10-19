//! Factory registry (`ProductionState.factories`) tests from the `world` level,
//! where `Simulation::new`, `advance_tick`, `state_hash`, `set_logic_order_for_test`
//! and the snapshot API are reachable: sweep order, the state-hash fold, snapshot
//! round trips, the per-step charge against the house wallet, cancel refunds and
//! the delivery-gated queue advance.

use super::Simulation;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
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
            .register(HouseState::new(owner, 0, None, true, 1_000_000, 10));
        let ty = sim.interner.intern("GRIZZLY");
        arm(&mut sim, &rules, owner, ProductionCategory::Vehicle, ty, 1);
        sim
    }
    let base = mid_build().state_hash();

    type FMut = fn(&mut crate::sim::production::Factory);
    let factory_muts: [FMut; 7] = [
        |f| f.progress += 1,
        |f| f.balance += 1,
        |f| f.step_timer = CdTimer::started(123, f.step_timer.duration()),
        |f| f.step_timer = CdTimer::from_raw(f.step_timer.start_frame(), 5),
        |f| f.on_hold = !f.on_hold,
        |f| f.suspended = !f.suspended,
        |f| f.step_rate_frames += 1,
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
        .register(HouseState::new(owner, 0, None, true, 1_000_000, 10));
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
        .register(HouseState::new(owner, 0, None, true, 1_000_000, 10));
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
    // This clone supplies the running step-kernel prior, after native Resume;
    // StartProduction alone leaves +70 suspended until that separate call.
    f.suspended = false;
    // The credits mirror is retired, so a cloned economy starts at 0 — fund the oracle
    // explicitly so the per-step charge can actually complete the build.
    let mut oracle = sim.houses[&owner].economy.clone();
    oracle.set_credits_for_test(700);
    assert!(
        (0..PRODUCTION_STEPS)
            .any(|_| { matches!(f.advance_one_step(&mut oracle), StepOutcome::Completed) }),
        "the running, funded head must complete within its native ladder"
    );
    assert!(
        f.suspended && f.object.is_some(),
        "C12: completion holds the object, suspended"
    );
    // The queue does NOT advance on completion alone (the cost is inert when the
    // object is still held — the guard fires before the seed).
    assert_eq!(
        f.start_next_queued(),
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
        f.start_next_queued(),
        Some(next),
        "after delivery the front pops"
    );
    assert!(
        f.object.is_none(),
        "forced Begin owns construction after the pop"
    );
    assert!(f.queue.is_empty(), "tail consumed");

    assert_eq!(
        before,
        sim.state_hash(),
        "the clone drive must not perturb the hash"
    );
}

// ===== P5a — flip-prep (pure producers + temporal mint + inversion-readiness, hash-neutral) =====

// ===== P5b — the authority flip: real-wallet charge guards (end-to-end via advance_tick) =====

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
            .register(HouseState::new(a, 0, None, true, 1_000_000, 10));
        sim.houses
            .register(HouseState::new(b, 1, None, true, 1_000_000, 10));
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
                    let _ = crate::sim::production::cancel_production_for_owner(
                        &mut sim,
                        &rules,
                        "Americans",
                        crate::sim::production::ProductionCancellation::Type {
                            type_id: "BEAG",
                            naval: false,
                        },
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
