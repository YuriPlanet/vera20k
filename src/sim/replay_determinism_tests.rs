//! Determinism and replay parity tests for fixed-step simulation.
//!
//! Migrated from `tests/determinism_replay.rs` (F12): the fixture entry points
//! (`Simulation::advance_tick`, `ReplayRunner::run_fixture`) are `#[cfg(test)]`
//! since F09, so this coverage must live inside the lib test harness.

use crate::map::entities::{EntityCategory, MapEntity};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::world::Simulation;

const TICK_MS: u32 = 33;

fn make_test_sim() -> Simulation {
    let mut sim = Simulation::with_seed(123_456);
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let entity = MapEntity {
        owner: "Americans".to_string(),
        type_id: "MTNK".to_string(),
        health: 256,
        cell_x: 2,
        cell_y: 2,
        facing: 64,
        category: EntityCategory::Unit,
        sub_cell: 0,
        veterancy: Some(0),
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
        trigger_tag: None,
    };
    sim.spawn_from_map(&[entity], None);
    sim
}

fn make_move_command() -> CommandEnvelope {
    CommandEnvelope::new(
        crate::sim::intern::test_intern("Americans"),
        1,
        Command::Move {
            entity_id: 1,
            target_rx: 12,
            target_ry: 2,
        },
    )
}

fn run_with_frame_profile(total_ms: u32, frame_ms: u32) -> (u64, Vec<Option<u64>>, ReplayLog) {
    let mut sim = make_test_sim();
    let grid = PathGrid::new(32, 32);
    let mut pending = vec![make_move_command()];
    let mut acc_ms: u64 = 0;
    let mut hashes: Vec<Option<u64>> = Vec::new();
    let mut replay = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: sim.session.pixel_conversion_bounds,
        version: 1,
        tick_hz: 30,
        seed: 123_456,
        map_name: "determinism_test".to_string(),
        rules_hash: 0,
    });

    let mut elapsed: u32 = 0;
    while elapsed < total_ms {
        let slice = frame_ms.min(total_ms - elapsed);
        elapsed += slice;
        acc_ms += slice as u64;

        while acc_ms >= TICK_MS as u64 {
            acc_ms -= TICK_MS as u64;
            let execute_tick = sim.session.tick + 1;
            let mut due: Vec<CommandEnvelope> = Vec::new();
            pending.retain(|cmd| {
                if cmd.execute_tick <= execute_tick {
                    due.push(cmd.clone());
                    false
                } else {
                    true
                }
            });

            let result = sim.advance_tick(&due, None, Some(&grid), None, TICK_MS);
            hashes.push(result.state_hash);
            replay.record_tick(result.tick, due, result.state_hash);
        }
    }

    (sim.state_hash(), hashes, replay)
}

#[test]
fn determinism_repeatability_same_inputs() {
    let (hash_a, timeline_a, _) = run_with_frame_profile(2500, 16);
    let (hash_b, timeline_b, _) = run_with_frame_profile(2500, 16);
    assert_eq!(hash_a, hash_b, "Final hash should be identical");
    assert_eq!(
        timeline_a, timeline_b,
        "Per-tick hash timeline should be identical"
    );
}

/// AT-2: playback is constructed FROM the recorded header seed and reproduces
/// the live timeline; a corrupted header seed (consistently applied) fails to
/// reproduce it. Proves the recorded seed is the playback authority.
#[test]
fn replay_reapplies_header_seed() {
    use crate::sim::scenario_session::ScenarioDescriptor;

    fn sim_with_unit(desc: &ScenarioDescriptor) -> Simulation {
        let mut sim = Simulation::from_descriptor(desc);
        // This synthetic fixture omits the production native Rules prefix.
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        let entity = MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 2,
            cell_y: 2,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: Some(0),
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
            trigger_tag: None,
        };
        sim.spawn_from_map(&[entity], None);
        sim
    }

    // Record under a descriptor seed. The map name is hashed session state,
    // so the fixture sets it on the descriptor exactly like the real launch
    // path does — the header then derives both fields from the session.
    let desc = ScenarioDescriptor {
        seed: 0x00A1_1CE5,
        map_name: "seed_roundtrip".to_string(),
        pixel_conversion_bounds: crate::util::pixel_conversion::PixelConversionBounds {
            width: 640,
            height: 480,
        },
        ..Default::default()
    };
    let mut sim = sim_with_unit(&desc);
    let grid = PathGrid::new(32, 32);
    let mut replay = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: sim.session.pixel_conversion_bounds,
        version: 1,
        tick_hz: 30,
        // The descriptor seed/map name are what the sim was constructed from;
        // recording them is exactly what the live header path does.
        seed: u64::from(desc.seed),
        map_name: desc.map_name.clone(),
        rules_hash: 0,
    });
    let mut live: Vec<Option<u64>> = Vec::new();
    let mut pending = vec![make_move_command()];
    for _ in 0..40 {
        let execute_tick = sim.session.tick + 1;
        let mut due: Vec<CommandEnvelope> = Vec::new();
        pending.retain(|cmd| {
            if cmd.execute_tick <= execute_tick {
                due.push(cmd.clone());
                false
            } else {
                true
            }
        });
        let r = sim.advance_tick(&due, None, Some(&grid), None, TICK_MS);
        replay.record_tick(r.tick, due, r.state_hash);
        live.push(r.state_hash);
    }

    // Playback constructed FROM the header — the descriptor contract.
    let descriptor_from_header = |header: &ReplayHeader| ScenarioDescriptor {
        seed: u32::try_from(header.seed).expect("diagnostic replay seed fits native width"),
        map_name: header.map_name.clone(),
        pixel_conversion_bounds: header.pixel_conversion_bounds,
        ..Default::default()
    };
    let mut playback = sim_with_unit(&descriptor_from_header(&replay.header));
    let replayed = ReplayRunner::run_fixture(&mut playback, &replay, None, Some(&grid), TICK_MS);
    assert_eq!(
        live, replayed,
        "playback from header.seed must match the recorded timeline"
    );

    // Corrupt the header: a consistent-but-wrong seed must diverge.
    let mut corrupted = replay.clone();
    corrupted.header.seed ^= 1;
    let mut wrong = sim_with_unit(&descriptor_from_header(&corrupted.header));
    let diverged = ReplayRunner::run_fixture(&mut wrong, &corrupted, None, Some(&grid), TICK_MS);
    assert_ne!(
        live, diverged,
        "a corrupted header seed must not reproduce the timeline"
    );
}
