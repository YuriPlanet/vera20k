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
use crate::sim::production::ProductionCategory;
use crate::sim::timer::CdTimer;

/// Rules with a costed buildable vehicle (`GRIZZLY`, Cost 700) so the per-step charge
/// machine actually moves credits. `BEAG` (Cost 600) gives a second category for the
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
/// resolved from `rules` (0 for a type the rules lack).
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
        .factories_mut()
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
        |f| f.set_progress_for_test(f.progress() + 1),
        |f| f.set_balance_for_test(f.balance() + 1),
        |f| f.set_step_timer_for_test(CdTimer::started(123, f.step_timer().duration())),
        |f| f.set_step_timer_for_test(CdTimer::from_raw(f.step_timer().start_frame(), 5)),
        |f| f.set_on_hold_for_test(!f.on_hold()),
        |f| f.set_suspended_for_test(!f.suspended()),
        |f| f.set_step_rate_frames_for_test(f.step_rate_frames() + 1),
    ];
    for m in factory_muts {
        let mut sim = mid_build();
        m(sim.production.factories_mut().test_first_mut().unwrap());
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
        m(sim.houses.get_mut(&owner).unwrap().economy_mut());
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
        let f = sim.production.factories_mut().test_first_mut().unwrap();
        f.set_progress_for_test(20);
        f.set_balance_for_test(300);
        f.set_step_timer_for_test(CdTimer::started(90, 12));
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
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

// ===== P4 — FIFO queue =====

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
        sim.production.set_next_enqueue_order_for_test(4);

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
