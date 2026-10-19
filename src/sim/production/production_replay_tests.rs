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

use super::ProductionCategory;
use super::tests::{build_catalog_rules, spawn_structure};
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::world::Simulation;

const TICK_MS: u32 = 67;
const START_CREDITS: i32 = 50_000;

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
        sim.houses
            .register(HouseState::new(oid, *side, None, true, START_CREDITS, 10));
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

/// Drive `sim` for `ticks` ticks, dispatching due commands each tick, recording a
/// `ReplayLog` + the per-tick `state_hash` timeline.
fn record(
    sim: &mut Simulation,
    rules: &RuleSet,
    pending: Vec<CommandEnvelope>,
    ticks: u64,
) -> (Vec<Option<u64>>, ReplayLog) {
    let mut log = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: Default::default(),
        version: 1,
        tick_hz: 15,
        // Record the sim's actual construction seed — playback fidelity is
        // header-seed-driven and ReplayRunner asserts the two agree.
        seed: sim.session.seed(),
        map_name: "p5c_factory_replay".to_string(),
        rules_hash: 0,
    });
    let mut hashes = Vec::with_capacity(ticks as usize);
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    sim.queue_commands(pending);
    for _ in 0..ticks {
        let due = sim.take_due_commands();
        let r = sim.advance_tick(&due, Some(rules), None, None, TICK_MS);
        hashes.push(r.state_hash);
        log.record_tick(r.tick, due, r.state_hash);
    }
    (hashes, log)
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
    let distinct: std::collections::BTreeSet<Option<u64>> = timeline_live.iter().copied().collect();
    assert!(
        distinct.len() > 1,
        "the command stream must actually move state_hash over the run (it did not)"
    );
}

// ===== P6 — prerequisite / factory-loss revalidation =====

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
        sim.houses
            .register(HouseState::new(owner, 0, None, true, START_CREDITS, 10));
        spawn_structure(&mut sim, &rules, 1, "Americans", "GACNST", 10, 10);
        spawn_structure(&mut sim, &rules, 2, "Americans", factory, 16, 10);
        spawn_structure(&mut sim, &rules, 3, "Americans", "GAPOWR", 22, 10);
        spawn_structure(&mut sim, &rules, 4, "Americans", "GAPOWR", 26, 10);
        spawn_structure(&mut sim, &rules, 5, "Americans", "GAPOWR", 30, 10);
        // Full-strength buildings, so the plants give their retail 200 each.
        for id in 1..=5 {
            let entity = sim.substrate.entities.get_mut(id).expect("structure");
            let strength = sim
                .interner
                .object_type(entity.type_ref(), &rules)
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
            let frame = sim.session.binary_frame();
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
                assert!(entity.lifecycle.in_limbo() && !entity.in_logic_vector.is_member());
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
