//! P5c — the Factory/House authority-flip replay/parity ACCEPTANCE GATE.
//!
//! The ratification of the P5b authority flip (the first hashed-state change).
//! Drives REAL production commands (`QueueProduction` / `SuspendProduction` /
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
//! the pre-flip charge path is retired at the flip. The determinism + conservation
//! gates here are the load-bearing ratification of the flip.
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
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::world::Simulation;

const TICK_MS: u32 = 67;
const START_CREDITS: i32 = 50_000;

#[test]
fn income_spending_and_factory_refund_share_the_runtime_wallet() {
    let (simulation, rules) = scenario();
    let (owner, _, _, tank) = ids(&simulation);
    let mut resources = crate::sim::runtime::SimResources::empty();
    resources.rules = rules;
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation,
        resources,
    };
    for tick in 1..=40 {
        if tick == 5 {
            crate::sim::credit_income::add_credits(&mut runtime.simulation, owner, 123);
        }
        if tick == 10 {
            crate::sim::credit_income::spend_money(&mut runtime.simulation, owner, 17);
        }
        let commands = match tick {
            1 => vec![queue(owner, tank, tick)],
            40 => vec![env(
                owner,
                tick,
                Command::CancelProductionByType {
                    type_id: tank,
                    all: false,
                },
            )],
            _ => Vec::new(),
        };
        runtime
            .advance_frame(&commands, TICK_MS, crate::sim::world::TickLane::Ordinary, crate::sim::world::FrameEffects::default())
            .expect("production frame");
        let cash = runtime.simulation.houses[&owner].economy.credits();
        assert_eq!(
            cash,
            super::credits_for_owner(&runtime.simulation, "Americans")
        );
        assert_eq!(
            cash,
            crate::sim::credit_income::available_money(&runtime.simulation, owner)
        );
        if tick == 39 {
            assert!(
                cash < START_CREDITS + 123 - 17,
                "the live factory charged the wallet"
            );
        }
    }
    let economy = &runtime.simulation.houses[&owner].economy;
    assert!(economy.spent_credits() > 0);
    assert_eq!(
        economy.credits(),
        START_CREDITS + 123 - 17,
        "active cancellation refunds every factory charge without losing intervening income/debits"
    );
}

/// Two funded human owners (Americans, Alliance), each owning a Construction Yard +
/// Barracks + War Factory + Air Force HQ, so the [`build_catalog_rules`] units
/// (E1 / MTNK / ORCA) are Strict-mode buildable. No power plant is needed — none of
/// these structures drains power, so the producer runs at full rate.
///
/// Fully deterministic so the cross-sim replay invariant holds (see module docs).
fn scenario() -> (Simulation, RuleSet) {
    let mut sim = Simulation::new();
    let rules = build_catalog_rules();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);

    // Both bases sit inside the delivery fixture map's In_Bounds diamond.
    let owners = [("Americans", 0u8, 10u16, 10u16), ("Alliance", 1u8, 10, 20)];
    for (i, (owner, side, base_x, base_y)) in owners.iter().enumerate() {
        let oid = sim.interner.intern(owner);
        sim.houses.insert(
            oid,
            HouseState::new(oid, *side, None, true, START_CREDITS, 10),
        );
        let sid = (i as u64) * 10 + 1;
        spawn_structure(&mut sim, &rules, sid, owner, "GACNST", *base_x, *base_y);
        spawn_structure(
            &mut sim,
            &rules,
            sid + 1,
            owner,
            "GAPILE",
            *base_x + 2,
            *base_y,
        );
        spawn_structure(
            &mut sim,
            &rules,
            sid + 2,
            owner,
            "GAWEAP",
            *base_x + 4,
            *base_y,
        );
        spawn_structure(
            &mut sim,
            &rules,
            sid + 3,
            owner,
            "GAAIRC",
            *base_x + 6,
            *base_y,
        );
    }
    (sim, rules)
}

fn env(owner: InternedId, tick: u64, payload: Command) -> CommandEnvelope {
    CommandEnvelope::new(owner, tick, payload)
}

fn queue(owner: InternedId, type_id: InternedId, tick: u64) -> CommandEnvelope {
    env(owner, tick, Command::QueueProduction { type_id })
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
/// infantry queued behind the first), a mid-build hold + resume (a PRODUCE of the held
/// type), and a mid-build cancel (partial-refund active-abandon).
fn rich_command_stream(sim: &Simulation) -> Vec<CommandEnvelope> {
    let (am, al, e1, mtnk) = ids(sim);
    vec![
        // tick 1 — same-tick two-Begin (different owners) + a Vehicle for Americans.
        queue(am, e1, 1),
        queue(al, e1, 1),
        queue(am, mtnk, 1),
        // tick 2 — a second infantry behind the first (FIFO tail) for Americans.
        queue(am, e1, 2),
        // hold Americans' Vehicle build, then resume it.
        env(
            am,
            8,
            Command::SuspendProduction {
                category: ProductionCategory::Vehicle,
            },
        ),
        queue(am, mtnk, 25),
        // Alliance cancels its active infantry mid-build (partial refund).
        env(
            al,
            12,
            Command::CancelProductionByType {
                type_id: e1,
                all: false,
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
        let r = sim.advance_tick(&due, Some(rules), None, None, TICK_MS);
        hashes.push(r.state_hash);
        log.record_tick(r.tick, due, r.state_hash);
    }
    (hashes, log)
}

/// A build started by a command is armed at that frame (the build start `0x004C9EA0`) and
/// first steps, charging the wallet, one rate later (`FactoryClass::AI 0x004C9B20`).
#[test]
fn event_tail_enqueue_first_charges_one_rate_later() {
    let (mut sim, rules) = scenario();
    let (owner, _, infantry, _) = ids(&sim);
    let credits_before = sim.houses[&owner].economy.credits();

    let start_frame = sim.session.binary_frame;
    sim.advance_tick(
        &[queue(owner, infantry, 1)],
        Some(&rules),
        None,
        None,
        TICK_MS,
    );
    let factory = |sim: &Simulation| {
        sim.production
            .factories
            .iter_insertion_ordered()
            .into_iter()
            .find(|f| f.owner == owner && f.category == ProductionCategory::Infantry)
            .map(|f| (f.progress, f.step_rate_frames, f.step_timer))
            .expect("the command tail arms the factory")
    };
    let (progress, rate, timer) = factory(&sim);
    assert_eq!(progress, 0);
    assert_eq!(timer.start_frame(), start_frame as i32);
    assert!(rate > 1);
    assert_eq!(sim.houses[&owner].economy.credits(), credits_before);

    for _ in 1..rate {
        sim.advance_tick(&[], Some(&rules), None, None, TICK_MS);
        assert_eq!(factory(&sim).0, 0);
    }
    assert_eq!(sim.houses[&owner].economy.credits(), credits_before);
    sim.advance_tick(&[], Some(&rules), None, None, TICK_MS);
    assert_eq!(factory(&sim).0, 1);
    assert!(sim.houses[&owner].economy.credits() < credits_before);
}

/// (P5d derived-state) An underfunded mid-build factory (on_hold) renders as Building in
/// the sidebar build queue, NOT "On Hold": a cash stall keeps the factory's rate and leaves
/// `+0x70` clear, so the strip's hold text (`0x006A9E9C..0x006A9ECC`) never shows for it.
/// The sibling of the blocked-exit Done case: the derived view state must reproduce the
/// exact observed label set {Building, Paused, Done, Queued}.
#[test]
fn derived_view_state_stays_building_on_underfunded_stall() {
    let (mut sim, rules) = scenario();
    let (am, _, e1, _) = ids(&sim);
    // Arm an E1 build directly, then simulate a mid-build underfunded stall.
    sim.production
        .factories
        .test_enqueue_kernel(am, ProductionCategory::Infantry, e1, 1, 200);
    {
        let f = sim
            .production
            .factories
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

/// Units and infantry that left their factory onto the map.
fn delivered_unit_count(sim: &Simulation) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| {
            matches!(
                e.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            ) && !e.lifecycle.in_limbo
        })
        .count()
}

/// (A) DETERMINISM — the lockstep ratification of the flip. A recorded production
/// command stream run live twice AND replayed through `ReplayRunner` yields a
/// bit-identical per-tick `state_hash` timeline.
#[test]
fn factory_flip_replay_is_bit_identical_across_runs_and_playback() {
    const TICKS: u64 = 120;

    // Run 1 — live record.
    let (mut s1, rules) = scenario();
    let cmds = rich_command_stream(&s1);
    let (timeline_live, log) = record(&mut s1, &rules, cmds.clone(), TICKS);

    // Run 2 — live record again (pure repeatability).
    let (mut s2, rules2) = scenario();
    let (timeline_live2, _) = record(&mut s2, &rules2, cmds, TICKS);
    assert_eq!(
        timeline_live, timeline_live2,
        "two live runs of the same command stream must produce an identical per-tick hash timeline"
    );

    // Run 3 — replay the recorded log through the shared ReplayRunner.
    let (mut s3, rules3) = scenario();
    let timeline_playback = ReplayRunner::run_fixture(&mut s3, &log, Some(&rules3), None, TICK_MS);
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

    let (mut sim, rules) = scenario();
    // The builds exit onto ground, so none is refunded for a failed delivery.
    super::tests::install_infantry_delivery_fixture_map(&mut sim);
    let (am, al, _, _) = ids(&sim);
    let pending = refund_free_stream(&sim);

    let initial: i64 = [am, al]
        .iter()
        .map(|o| sim.houses[o].economy.credits() as i64)
        .sum();

    sim.queue_commands(pending);
    let mut any_spent = false;
    for _ in 0..TICKS {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), None, None, TICK_MS);

        let total: i64 = [am, al]
            .iter()
            .map(|o| {
                let h = &sim.houses[o];
                h.economy.credits() as i64 + h.economy.spent_credits() as i64
            })
            .sum();
        assert_eq!(
            total, initial,
            "tick {}: credits+spent_credits must equal the initial pool (no money created/destroyed)",
            sim.session.tick
        );
        if [am, al]
            .iter()
            .any(|o| sim.houses[o].economy.spent_credits() > 0)
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
/// mid-build active object refunds its Cost_Of less the Balance still owed, which with
/// no FactoryPlant change is exactly the already-charged portion, back to the one
/// wallet and nowhere else. The
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

    let (mut sim, rules) = scenario();
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
                type_id: mtnk,
                all: false,
            },
        ),
    ]);

    let initial: i64 = [am, al]
        .iter()
        .map(|o| sim.houses[o].economy.credits() as i64)
        .sum();
    let mtnk_cost = sim
        .object_type(mtnk, &rules)
        .map(|o| o.cost.max(0))
        .expect("MTNK has a cost") as i64;

    let mut prev: BTreeMap<InternedId, i32> = [am, al]
        .iter()
        .map(|&o| (o, sim.houses[&o].economy.credits()))
        .collect();
    let mut cumulative_refunded: i64 = 0;

    for _ in 0..TICKS {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), None, None, TICK_MS);

        // Every per-owner credit INCREASE is a refund (no deposits in this scenario).
        for &o in &[am, al] {
            let now = sim.houses[&o].economy.credits();
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
                h.economy.credits() as i64 + h.economy.spent_credits() as i64
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
            .factories
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
        .factories
        .test_enqueue_kernel(am, ProductionCategory::Vehicle, mtnk, 1, 900);
    sim.production
        .factories
        .test_enqueue_kernel(am, ProductionCategory::Vehicle, mtnk, 2, 900);
    assert!(
        sim.production
            .factories
            .view(am, ProductionCategory::Vehicle)
            .is_some()
    );
    sim.advance_tick(&[], Some(&rules), None, None, TICK_MS);
    assert!(
        sim.production
            .factories
            .view(am, ProductionCategory::Vehicle)
            .is_none(),
        "no-factory build abandoned + queued dropped -> factory pruned"
    );
    assert_eq!(
        sim.houses[&am].economy.credits(),
        50_000,
        "an uncharged abandon refunds nothing (credits unchanged)"
    );
}

/// A build whose producing factory is DESTROYED mid-progress is abandoned with the C8 PARTIAL
/// refund (Cost_Of - Balance: exactly the already-charged portion) into
/// house.economy.credits, and the factory is pruned. Revalidation runs before the charge
/// sweep, so no extra charge lands the abandon tick.
#[test]
fn revalidate_abandons_active_on_factory_loss_partial_refund() {
    let (mut sim, rules) = scenario();
    let (am, _, _, mtnk) = ids(&sim);
    // Enqueue MTNK via the real command path (eligible — Americans owns GAWEAP), then charge
    // partway.
    sim.queue_command(queue(am, mtnk, 1));
    for _ in 0..40 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), None, None, TICK_MS);
    }
    // Mid-build before factory loss; read the spent portion = the expected refund.
    let spent = {
        let f = sim
            .production
            .factories
            .test_factory_mut(am, ProductionCategory::Vehicle)
            .expect("active MTNK factory");
        assert!(
            f.object.is_some() && f.progress > 0 && f.progress < 54,
            "MTNK must be mid-build (active, in-progress) before factory loss"
        );
        900 - f.balance
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
            .is_some_and(|e| e.type_ref() == gaweap && e.owner() == am),
        "stable_id 3 is Americans' GAWEAP in scenario()"
    );
    sim.substrate.entities.remove(3);

    let credits_before = sim.houses[&am].economy.credits();
    sim.advance_tick(&[], Some(&rules), None, None, TICK_MS);
    let refund = sim.houses[&am].economy.credits() - credits_before;
    assert_eq!(
        refund, spent,
        "factory-loss abandon refunds exactly the already-charged portion"
    );
    assert!(
        sim.production
            .factories
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
    let (mut sim, rules) = scenario();
    let (am, _, _, mtnk) = ids(&sim);
    sim.queue_command(queue(am, mtnk, 1));
    for _ in 0..40 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), None, None, TICK_MS);
    }
    let view = sim
        .production
        .factories
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

    let (mut live, rules) = scenario();
    let cmds = rich_command_stream(&live);
    let (timeline_live, log) = record(&mut live, &rules, cmds, TICKS);

    let (fresh, runtime_rules) = scenario();
    let mut runtime = crate::sim::runtime::SimRuntime {
        simulation: fresh,
        resources: {
            let mut resources = crate::sim::runtime::SimResources::empty();
            resources.rules = runtime_rules;
            resources
        },
    };
    let timeline_runtime = ReplayRunner::run_runtime(&mut runtime, &log, TICK_MS).unwrap();
    assert_eq!(
        timeline_live, timeline_runtime,
        "runtime-backed replay must reproduce the live hash timeline bit-for-bit"
    );
}

/// Retail MTNK, FV and E1 through the production path. The queue command starts
/// the build, and its rate and steps match the originals' (`0x004C9EA0`, then
/// `FactoryClass::AI` `0x004C9B20` each frame) for the inputs the factory
/// resolved: the cadence oracle's funded row with those inputs, offset to the
/// build's start frame. This comparison ends with the held stage54 object,
/// matching the native cadence receiver; delivery is covered by the separate
/// joined output/admission comparisons, which supply actual map terrain.
#[test]
fn retail_builds_step_at_the_native_frames() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini(&rules_ini).expect("retail rules");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let cadence = super::factory::native_factory_cadence();
    let oracle_start = cadence["start_frame"].as_u64().unwrap() as u32;
    for (unit, factory, category) in [
        ("MTNK", "GAWEAP", ProductionCategory::Vehicle),
        ("FV", "GAWEAP", ProductionCategory::Vehicle),
        ("E1", "GAPILE", ProductionCategory::Infantry),
    ] {
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, None, true, START_CREDITS, 10),
        );
        spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
        spawn_structure(&mut sim, &rules, 2, "Americans", factory, 16, 10);
        spawn_structure(&mut sim, &rules, 3, "Americans", "GAPOWR", 22, 10);
        spawn_structure(&mut sim, &rules, 4, "Americans", "GAPOWR", 26, 10);
        spawn_structure(&mut sim, &rules, 5, "Americans", "GAPOWR", 30, 10);
        // Full-strength buildings, so the plants give their retail 200 each.
        for id in 1..=5 {
            let entity = sim.substrate.entities.get_mut(id).expect("structure");
            let strength = rules
                .object(sim.interner.resolve(entity.type_ref()))
                .expect("retail building")
                .strength;
            entity.health.set_for_test(strength);
        }
        let type_id = sim.interner.intern(unit);

        let factory_state = |sim: &Simulation| {
            sim.production
                .factories
                .iter_insertion_ordered()
                .into_iter()
                .find(|f| f.owner == owner && f.category == category)
                .map(|f| {
                    let held = f.object.as_ref().and_then(|object| object.entity_id);
                    (f.progress, f.step_rate_frames, f.step_timer, held)
                })
        };
        let mut start = None;
        let mut held = None;
        let mut step_frames = Vec::new();
        for tick in 1..=3000u64 {
            let frame = sim.session.binary_frame;
            let commands = if tick == 1 {
                vec![queue(owner, type_id, 1)]
            } else {
                Vec::new()
            };
            let before = factory_state(&sim).map_or(0, |(progress, ..)| progress);
            sim.advance_tick(&commands, Some(&rules), None, None, TICK_MS);
            let Some((progress, rate, timer, object)) = factory_state(&sim) else {
                assert!(
                    start.is_none(),
                    "{unit}: funded head disappeared before stage54"
                );
                continue;
            };
            if start.is_none() {
                assert_eq!(
                    timer.start_frame(),
                    frame as i32,
                    "{unit}: the build start arms the timer"
                );
                start = Some((frame, rate));
                held = object;
            }
            if progress != before {
                step_frames.push(frame);
            }
            if progress == super::factory::PRODUCTION_STEPS {
                // The original cadence oracle stops at Factory completion.
                // Keep the same limbo head here; Strip's later PLACE requires
                // terrain/admission priors this timer fixture does not supply.
                assert_eq!(
                    object, held,
                    "{unit}: completion preserves the held identity"
                );
                let entity = sim
                    .substrate
                    .entities
                    .get(held.unwrap())
                    .expect("held head");
                assert!(entity.lifecycle.in_limbo && !entity.in_logic_vector);
                break;
            }
        }
        let (start_frame, rate) = start.expect("the build started");
        let obj = sim.object_type(type_id, &rules).expect("retail type");
        let inputs = super::factory::time_to_build_inputs(&sim, &rules, owner, obj);
        let row = cadence["builds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                super::factory::native_time_to_build_inputs(row) == inputs
                    && row["final"]["stage"] == 54
            })
            .unwrap_or_else(|| panic!("{unit}: no funded cadence row for {inputs:?}"));
        assert_eq!(
            u64::from(rate),
            row["after_start"]["rate"].as_u64().unwrap(),
            "{unit}: the start's rate"
        );
        let expected: Vec<u32> = row["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|attempt| start_frame + (attempt[0].as_u64().unwrap() as u32 - oracle_start))
            .collect();
        assert_eq!(step_frames, expected, "{unit}: the native step frames");
    }
}
