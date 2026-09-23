//! P5c — the Factory/House authority-flip replay/parity ACCEPTANCE GATE.
//!
//! The ratification of the P5b authority flip (the first hashed-state change).
//! Drives REAL production commands (`QueueProduction` / `TogglePauseProduction` /
//! `CancelProductionByType`) through `advance_tick` and the shared replay harness
//! (`ReplayLog` / `ReplayRunner`), and asserts:
//!
//!   - (A) DETERMINISM — a recorded production command stream, run live TWICE and
//!     replayed once through `ReplayRunner`, yields a bit-identical per-tick
//!     `state_hash` timeline. This is the lockstep ratification of the flip: the
//!     newly-hashed `Factory`/`Economy` state machine adds no nondeterminism.
//!   - (B) CONSERVATION (C15) — over a refund-free replay, EVERY tick conserves
//!     `Σ_owners(house.economy.credits + economy.spent_credits) == Σ_owners(initial)`.
//!     The per-step charge moves credits out of the one wallet into `spent_credits`
//!     and nowhere else; no credit is created or destroyed by the charge machinery.
//!
//! Pre-flip-baseline observable equivalence (P5c part C) is intentionally DEFERRED:
//! the pre-flip charge path is retired at the flip, and the one intended difference
//! (the x0.9-free producer cadence) is already documented and asserted by the
//! producer's own tests. The determinism + conservation gates here are the
//! load-bearing ratification of the flip.
//!
//! Cross-sim replay invariant: a command carries owner/type as `InternedId`, and
//! `advance_tick` resolves those IDs against the sim's OWN interner. `scenario()`
//! is fully deterministic (interns rules → types → owners in a fixed order), so any
//! two sims it produces have identical interner state — an `InternedId` minted
//! against one is valid in the other. Both the recording sim and the replay sim are
//! built by `scenario()`, so the recorded `CommandEnvelope`s resolve correctly on
//! playback.

use std::collections::BTreeMap;

use super::tests::{build_catalog_rules, spawn_structure};
use super::{BuildQueueState, ProductionCategory, queue_view_for_owner};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope, QueueMode};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::world::Simulation;

const TICK_MS: u32 = 67;
const START_CREDITS: i32 = 50_000;

#[test]
fn income_spending_and_factory_refund_share_the_runtime_wallet() {
    let (simulation, rules, height_map) = scenario();
    let (owner, _, _, tank) = ids(&simulation);
    let mut resources = crate::sim::runtime::SimResources::empty();
    resources.rules = rules;
    resources.height_map = height_map;
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation,
        resources,
    };
    for tick in 1..=40 {
        if tick == 5 {
            crate::sim::credit_income::add_credits(&mut runtime.simulation, owner, 123);
        }
        if tick == 10 {
            assert_eq!(
                crate::sim::credit_income::spend_money(&mut runtime.simulation, owner, 17),
                17
            );
        }
        let commands = match tick {
            1 => vec![queue(owner, tank, tick)],
            40 => vec![env(
                owner,
                tick,
                Command::CancelProductionByType {
                    owner,
                    type_id: tank,
                },
            )],
            _ => Vec::new(),
        };
        runtime
            .advance_frame(&commands, TICK_MS, crate::sim::world::TickLane::Ordinary)
            .expect("production frame");
        let cash = runtime.simulation.houses[&owner].economy.credits;
        assert_eq!(
            cash,
            super::credits_for_owner(&runtime.simulation, "Americans")
        );
        assert_eq!(
            cash,
            crate::sim::credit_income::available_money(&runtime.simulation, owner)
        );
        if tick == 2 {
            assert!(cash < START_CREDITS, "the live factory charged the wallet");
        }
    }
    let economy = &runtime.simulation.houses[&owner].economy;
    assert!(economy.spent_credits > 0);
    assert_eq!(
        economy.credits,
        START_CREDITS + 123 - 17,
        "active cancellation refunds every factory charge without losing intervening income/debits"
    );
}

/// Two funded human owners (Americans, Alliance), each owning a Construction Yard
/// + Barracks + War Factory + Air Force HQ, so the [`build_catalog_rules`] units
/// (E1 / MTNK / ORCA) are Strict-mode buildable. No power plant is needed — none of
/// these structures drains power, so the producer runs at full rate.
///
/// Fully deterministic so the cross-sim replay invariant holds (see module docs).
fn scenario() -> (Simulation, RuleSet, BTreeMap<(u16, u16), u8>) {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    let owners = [("Americans", 0u8, 10u16), ("Alliance", 1u8, 30u16)];
    for (i, (owner, side, base_x)) in owners.iter().enumerate() {
        let oid = sim.interner.intern(owner);
        sim.houses.insert(
            oid,
            HouseState::new(oid, *side, None, true, START_CREDITS, 10),
        );
        let sid = (i as u64) * 10 + 1;
        spawn_structure(&mut sim, sid, owner, "GACNST", *base_x, 10);
        spawn_structure(&mut sim, sid + 1, owner, "GAPILE", *base_x + 2, 10);
        spawn_structure(&mut sim, sid + 2, owner, "GAWEAP", *base_x + 4, 10);
        spawn_structure(&mut sim, sid + 3, owner, "GAAIRC", *base_x + 6, 10);
        // `spawn_structure` is a raw test helper and intentionally bypasses
        // construction's Add_Tracking.  Keep this replay fixture in a live match
        // so late defeat handling cannot freeze its command ordinal.
        sim.houses
            .get_mut(&oid)
            .expect("scenario house exists")
            .tracking
            .set_buildings_for_test(4);
    }
    (sim, rules, BTreeMap::new())
}

fn env(owner: InternedId, tick: u64, payload: Command) -> CommandEnvelope {
    CommandEnvelope::new(owner, tick, payload)
}

fn queue(owner: InternedId, type_id: InternedId, tick: u64) -> CommandEnvelope {
    env(
        owner,
        tick,
        Command::QueueProduction {
            owner,
            type_id,
            mode: QueueMode::Append,
        },
    )
}

/// Resolve the owner/type IDs the streams reference. All are already interned by
/// `scenario()`, so `.get()` never returns `None` and never mints a new ID.
fn ids(sim: &Simulation) -> (InternedId, InternedId, InternedId, InternedId) {
    (
        sim.interner.get("Americans").expect("Americans interned"),
        sim.interner.get("Alliance").expect("Alliance interned"),
        sim.interner.get("E1").expect("E1 interned"),
        sim.interner.get("MTNK").expect("MTNK interned"),
    )
}

/// A rich stream exercising the full hashed-state machinery: a same-tick two-Begin
/// from two owners, a second category (Vehicle) for one owner, a FIFO tail (a second
/// infantry queued behind the first), a mid-build pause + resume, and a mid-build
/// cancel (partial-refund active-abandon).
fn rich_command_stream(sim: &Simulation) -> Vec<CommandEnvelope> {
    let (am, al, e1, mtnk) = ids(sim);
    vec![
        // tick 1 — same-tick two-Begin (different owners) + a Vehicle for Americans.
        queue(am, e1, 1),
        queue(al, e1, 1),
        queue(am, mtnk, 1),
        // tick 2 — a second infantry behind the first (FIFO tail) for Americans.
        queue(am, e1, 2),
        // pause Americans' Vehicle build, then resume.
        env(
            am,
            8,
            Command::TogglePauseProduction {
                owner: am,
                category: ProductionCategory::Vehicle,
            },
        ),
        env(
            am,
            25,
            Command::TogglePauseProduction {
                owner: am,
                category: ProductionCategory::Vehicle,
            },
        ),
        // Alliance cancels its active infantry mid-build (partial refund).
        env(
            al,
            12,
            Command::CancelProductionByType {
                owner: al,
                type_id: e1,
            },
        ),
    ]
}

/// A refund-free stream (only Begins) for the conservation gate: with no cancels,
/// no refund ever fires, so `credits + spent_credits` is exactly conserved.
fn refund_free_stream(sim: &Simulation) -> Vec<CommandEnvelope> {
    let (am, al, e1, mtnk) = ids(sim);
    vec![
        queue(am, e1, 1),
        queue(al, e1, 1),
        queue(am, mtnk, 1),
        queue(al, mtnk, 2),
        queue(am, e1, 2), // a tail behind the first infantry
    ]
}

/// Drive `sim` for `ticks` ticks, dispatching due commands each tick, recording a
/// `ReplayLog` + the per-tick `state_hash` timeline.
fn record(
    sim: &mut Simulation,
    rules: &RuleSet,
    heights: &BTreeMap<(u16, u16), u8>,
    pending: Vec<CommandEnvelope>,
    ticks: u64,
) -> (Vec<u64>, ReplayLog) {
    let mut log = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: Default::default(),
        version: 1,
        tick_hz: 15,
        // Record the sim's actual construction seed — playback fidelity is
        // header-seed-driven and ReplayRunner asserts the two agree.
        seed: sim.session.seed,
        map_name: "p5c_factory_replay".to_string(),
        rules_hash: 0,
    });
    let mut hashes = Vec::with_capacity(ticks as usize);
    sim.queue_commands(pending);
    for _ in 0..ticks {
        let due = sim.take_due_commands();
        let r = sim.advance_tick(&due, Some(rules), heights, None, None, TICK_MS);
        hashes.push(r.state_hash);
        log.record_tick(r.tick, due, r.state_hash);
    }
    (hashes, log)
}

#[test]
fn event_tail_enqueue_first_charges_on_the_following_frame() {
    let (mut sim, rules, heights) = scenario();
    let (owner, _, infantry, _) = ids(&sim);
    let credits_before = sim.houses[&owner].economy.credits;

    sim.advance_tick(
        &[queue(owner, infantry, 1)],
        Some(&rules),
        &heights,
        None,
        None,
        TICK_MS,
    );
    let armed = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .expect("the command tail arms the factory");
    assert_eq!(armed.progress, 0);
    assert_eq!(sim.houses[&owner].economy.credits, credits_before);

    sim.advance_tick(&[], Some(&rules), &heights, None, None, TICK_MS);
    let charged = sim
        .production
        .factory_shadow
        .view(owner, ProductionCategory::Infantry)
        .expect("the active factory remains registered");
    assert_eq!(charged.progress, 1);
    assert!(sim.houses[&owner].economy.credits < credits_before);
}

/// (P5d derived-state) An underfunded mid-build factory (on_hold) renders as Building in
/// the sidebar build queue, NOT "On Hold"/NoFunds — the pre-P5d front never surfaced NoFunds
/// during a stall (it stayed Building; on_hold is internal). The sibling of the blocked-exit
/// Done case: the derived view state must reproduce the exact observed label set
/// {Building, Paused, Done, Queued}.
#[test]
fn derived_view_state_stays_building_on_underfunded_stall() {
    let (mut sim, rules, _heights) = scenario();
    let (am, _, e1, _) = ids(&sim);
    // Arm an E1 build directly, then simulate a mid-build underfunded stall.
    sim.production
        .factory_shadow
        .enqueue(am, ProductionCategory::Infantry, e1, 1, 100, 200);
    {
        let f = sim
            .production
            .factory_shadow
            .test_factory_mut(am, ProductionCategory::Infantry)
            .expect("infantry factory armed");
        f.progress = 10;
        f.on_hold = true; // underfunded stall: not paused, not complete
    }
    let view = queue_view_for_owner(&sim, &rules, "Americans");
    assert_eq!(
        view.len(),
        1,
        "the single armed build projects to one view item"
    );
    assert_eq!(
        view[0].state,
        BuildQueueState::Building,
        "an on_hold (underfunded) stall renders Building, never NoFunds"
    );
}

fn delivered_unit_count(sim: &Simulation) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| matches!(e.category, EntityCategory::Unit | EntityCategory::Infantry))
        .count()
}

/// (A) DETERMINISM — the lockstep ratification of the flip. A recorded production
/// command stream run live twice AND replayed through `ReplayRunner` yields a
/// bit-identical per-tick `state_hash` timeline.
#[test]
fn factory_flip_replay_is_bit_identical_across_runs_and_playback() {
    const TICKS: u64 = 120;

    // Run 1 — live record.
    let (mut s1, rules, heights) = scenario();
    let cmds = rich_command_stream(&s1);
    let (timeline_live, log) = record(&mut s1, &rules, &heights, cmds.clone(), TICKS);

    // Run 2 — live record again (pure repeatability).
    let (mut s2, rules2, heights2) = scenario();
    let (timeline_live2, _) = record(&mut s2, &rules2, &heights2, cmds, TICKS);
    assert_eq!(
        timeline_live, timeline_live2,
        "two live runs of the same command stream must produce an identical per-tick hash timeline"
    );

    // Run 3 — replay the recorded log through the shared ReplayRunner.
    let (mut s3, rules3, heights3) = scenario();
    let timeline_playback =
        ReplayRunner::run_fixture(&mut s3, &log, Some(&rules3), &heights3, None, TICK_MS);
    assert_eq!(
        timeline_live, timeline_playback,
        "replay playback must reproduce the live hash timeline bit-for-bit"
    );

    // The gate must be exercising hashed state, not asserting on a no-op.
    let distinct: std::collections::BTreeSet<u64> = timeline_live.iter().copied().collect();
    assert!(
        distinct.len() > 1,
        "the command stream must actually move state_hash over the run (it did not)"
    );
}

/// (B) CONSERVATION (C15) — over a refund-free replay, every tick conserves the
/// global money pool: `Σ(house.economy.credits + economy.spent_credits) == Σ(initial)`.
/// The per-step charge is the only mover of credits; nothing is created or lost.
#[test]
fn economy_conservation_over_replay() {
    const TICKS: u64 = 600;

    let (mut sim, rules, heights) = scenario();
    let (am, al, _, _) = ids(&sim);
    let pending = refund_free_stream(&sim);

    let initial: i64 = [am, al]
        .iter()
        .map(|o| sim.houses[o].economy.credits as i64)
        .sum();

    sim.queue_commands(pending);
    let mut any_spent = false;
    for _ in 0..TICKS {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), &heights, None, None, TICK_MS);

        let total: i64 = [am, al]
            .iter()
            .map(|o| {
                let h = &sim.houses[o];
                h.economy.credits as i64 + h.economy.spent_credits as i64
            })
            .sum();
        assert_eq!(
            total, initial,
            "tick {}: credits+spent_credits must equal the initial pool (no money created/destroyed)",
            sim.session.tick
        );
        if [am, al]
            .iter()
            .any(|o| sim.houses[o].economy.spent_credits > 0)
        {
            any_spent = true;
        }
    }

    assert!(
        any_spent,
        "the per-step charge must actually have moved credits over the replay"
    );
    assert!(
        delivered_unit_count(&sim) >= 1,
        "at least one build must complete and deliver over the replay (the full charge cycle)"
    );
}

/// (B') CONSERVATION THROUGH THE PARTIAL-REFUND BRANCH (C8/C15) — the cancel of a
/// mid-build active object refunds exactly the already-charged portion
/// (`original_balance − balance`) back to the one wallet and nowhere else. The
/// global pool is conserved once refunds are accounted for:
/// `Σ(credits + spent_credits) − cumulative_refunded == initial` at every tick.
///
/// `cumulative_refunded` is measured independently of the engine — every per-owner
/// credit INCREASE is a refund (this scenario has no deposits/income), so summing
/// the positive per-owner deltas reconstructs total refunded without reading factory
/// internals. The cancelling owner builds ONLY the cancelled item, so on the cancel
/// tick its sole credit movement is the refund (the build is removed before the
/// Phase-7 charge sweep), keeping the per-owner delta a clean refund signal.
#[test]
fn economy_conservation_through_cancel_refund() {
    const TICKS: u64 = 300;
    // EventClass dispatch runs after the production sweep, and a newly armed
    // factory first charges on the following gameplay frame.  Tick 40 is far
    // enough into the stock step cadence to guarantee a partial (not zero/full)
    // MTNK balance when the cancel reaches the command tail.
    const CANCEL_TICK: u64 = 40;

    let (mut sim, rules, heights) = scenario();
    let (am, al, e1, mtnk) = ids(&sim);

    // Americans: ONLY an MTNK (Cost 900 -> guaranteed mid-build at the cancel tick),
    // cancelled mid-build. Alliance: an E1 + an MTNK that run uninterrupted.
    sim.queue_commands(vec![
        queue(am, mtnk, 1),
        queue(al, e1, 1),
        queue(al, mtnk, 2),
        env(
            am,
            CANCEL_TICK,
            Command::CancelProductionByType {
                owner: am,
                type_id: mtnk,
            },
        ),
    ]);

    let initial: i64 = [am, al]
        .iter()
        .map(|o| sim.houses[o].economy.credits as i64)
        .sum();
    let mtnk_cost = sim
        .object_type(mtnk, &rules)
        .map(|o| o.cost.max(0))
        .expect("MTNK has a cost") as i64;

    let mut prev: BTreeMap<InternedId, i32> = [am, al]
        .iter()
        .map(|&o| (o, sim.houses[&o].economy.credits))
        .collect();
    let mut cumulative_refunded: i64 = 0;

    for _ in 0..TICKS {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), &heights, None, None, TICK_MS);

        // Every per-owner credit INCREASE is a refund (no deposits in this scenario).
        for &o in &[am, al] {
            let now = sim.houses[&o].economy.credits;
            let delta = now - *prev.get(&o).unwrap();
            if delta > 0 {
                cumulative_refunded += delta as i64;
            }
            prev.insert(o, now);
        }

        let pool: i64 = [am, al]
            .iter()
            .map(|o| {
                let h = &sim.houses[o];
                h.economy.credits as i64 + h.economy.spent_credits as i64
            })
            .sum();
        assert_eq!(
            pool - cumulative_refunded,
            initial,
            "tick {}: pool minus refunds must equal the initial pool (conservation through cancel)",
            sim.session.tick
        );
    }

    // The refund fired and was PARTIAL (mid-build), not the full cost — the C8 fix
    // (the legacy `.rev()` full refund is the retired DRIFT).
    assert!(
        cumulative_refunded > 0 && cumulative_refunded < mtnk_cost,
        "the cancel must refund the partial charged portion (0 < {} < {})",
        cumulative_refunded,
        mtnk_cost
    );
    // Americans' MTNK left the queue-of-record on cancel (the registry factory is gone —
    // P5d: the queue-of-record lives in the registry, pruned when idle).
    assert!(
        sim.production
            .factory_shadow
            .view(am, ProductionCategory::Vehicle)
            .is_none(),
        "the cancelled active build left the Americans Vehicle factory"
    );
}

// ===== P6 — prerequisite / factory-loss revalidation =====

/// A build with NO producing factory (NoFactory -> PermanentlyBlocked) is abandoned and its
/// queued tail dropped on the next tick's revalidation (which runs at the Phase-7 head, before
/// the charge sweep). An uncharged abandon refunds nothing; active + queued both disposed ->
/// the factory is pruned.
#[test]
fn revalidate_abandons_build_with_no_factory_and_drops_queued() {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let am = sim.interner.intern("Americans");
    sim.houses
        .insert(am, HouseState::new(am, 0, None, true, 50_000, 10));
    let mtnk = sim.interner.intern("MTNK");
    // Arm directly (bypassing the enqueue eligibility gate) an active + one queued MTNK for
    // an owner with NO war factory -> both classify NoFactory -> PermanentlyBlocked.
    sim.production
        .factory_shadow
        .enqueue(am, ProductionCategory::Vehicle, mtnk, 1, 100, 900);
    sim.production
        .factory_shadow
        .enqueue(am, ProductionCategory::Vehicle, mtnk, 2, 100, 900);
    assert!(
        sim.production
            .factory_shadow
            .view(am, ProductionCategory::Vehicle)
            .is_some()
    );
    let heights: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    sim.advance_tick(&[], Some(&rules), &heights, None, None, TICK_MS);
    assert!(
        sim.production
            .factory_shadow
            .view(am, ProductionCategory::Vehicle)
            .is_none(),
        "no-factory build abandoned + queued dropped -> factory pruned"
    );
    assert_eq!(
        sim.houses[&am].economy.credits, 50_000,
        "an uncharged abandon refunds nothing (credits unchanged)"
    );
}

/// A build whose producing factory is DESTROYED mid-progress is abandoned with the C8 PARTIAL
/// refund (exactly the already-charged portion `original_balance - balance`) into house.economy.credits,
/// and the factory is pruned. Revalidation runs before the charge sweep, so no extra charge
/// lands the abandon tick.
#[test]
fn revalidate_abandons_active_on_factory_loss_partial_refund() {
    let (mut sim, rules, heights) = scenario();
    let (am, _, _, mtnk) = ids(&sim);
    // Enqueue MTNK via the real command path (eligible — Americans owns GAWEAP), then charge
    // partway.
    sim.queue_command(queue(am, mtnk, 1));
    for _ in 0..40 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), &heights, None, None, TICK_MS);
    }
    // Mid-build before factory loss; read the spent portion = the expected refund.
    let spent = {
        let f = sim
            .production
            .factory_shadow
            .test_factory_mut(am, ProductionCategory::Vehicle)
            .expect("active MTNK factory");
        assert!(
            f.object.is_some() && f.progress > 0 && f.progress < 54,
            "MTNK must be mid-build (active, in-progress) before factory loss"
        );
        f.original_balance - f.balance
    };
    assert!(
        spent > 0 && spent < 900,
        "some but not all of the cost is charged"
    );

    // Destroy the war factory. In scenario() Americans' GAWEAP is stable_id 3.
    let gaweap = sim.interner.intern("GAWEAP");
    assert!(
        sim.substrate
            .entities
            .get(3)
            .is_some_and(|e| e.type_ref == gaweap && e.owner == am),
        "stable_id 3 is Americans' GAWEAP in scenario()"
    );
    sim.substrate.entities.remove(3);

    let credits_before = sim.houses[&am].economy.credits;
    sim.advance_tick(&[], Some(&rules), &heights, None, None, TICK_MS);
    let refund = sim.houses[&am].economy.credits - credits_before;
    assert_eq!(
        refund, spent,
        "factory-loss abandon refunds exactly the already-charged portion"
    );
    assert!(
        sim.production
            .factory_shadow
            .view(am, ProductionCategory::Vehicle)
            .is_none(),
        "the abandoned factory is pruned"
    );
}

/// Revalidation is a no-op on a buildable build: with factory + prereqs intact it is NOT
/// abandoned and keeps progressing. Pins that revalidation never over-abandons (the
/// hash-neutral steady state).
#[test]
fn revalidate_keeps_buildable_build_untouched() {
    let (mut sim, rules, heights) = scenario();
    let (am, _, _, mtnk) = ids(&sim);
    sim.queue_command(queue(am, mtnk, 1));
    for _ in 0..40 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), &heights, None, None, TICK_MS);
    }
    let view = sim
        .production
        .factory_shadow
        .view(am, ProductionCategory::Vehicle)
        .expect("buildable MTNK is still building, not abandoned");
    assert!(
        view.object.is_some(),
        "a buildable build is never abandoned by revalidation"
    );
    assert!(view.progress > 0, "and keeps progressing");
}

/// F09 matrix: runtime-backed replay hashes match each tick. The same
/// recorded log replayed through `ReplayRunner::run_runtime` — the bound
/// SimRuntime path headless and future callers use — reproduces the live
/// per-tick hash timeline bit-for-bit.
#[test]
fn runtime_backed_replay_hashes_match_each_tick() {
    const TICKS: u64 = 120;

    let (mut live, rules, heights) = scenario();
    let cmds = rich_command_stream(&live);
    let (timeline_live, log) = record(&mut live, &rules, &heights, cmds, TICKS);

    let (fresh, runtime_rules, runtime_heights) = scenario();
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation: fresh,
        resources: {
            let mut resources = crate::sim::runtime::SimResources::empty();
            resources.rules = runtime_rules;
            resources.height_map = runtime_heights;
            resources
        },
    };
    let timeline_runtime = ReplayRunner::run_runtime(&mut runtime, &log, TICK_MS).unwrap();
    assert_eq!(
        timeline_live, timeline_runtime,
        "runtime-backed replay must reproduce the live hash timeline bit-for-bit"
    );
}
