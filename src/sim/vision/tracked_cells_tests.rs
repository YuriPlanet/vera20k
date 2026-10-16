//! Rust regression tests for the tracked frame passes ([`super::TrackedCells`]
//! and the skipped unchanged sources): a battle's fog is the fog that
//! visiting every cell and footprint gives. No native behavior is claimed.
use crate::rules::retail_ini_fixture::retail_unlayered_rules;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::world::{Simulation, UninitContext};

/// Transient sight between frames: every 25th frame the Americans see 3 cells
/// around the first Russian (FireAt's RevealOnFire leaf), and every 60th an
/// area reveal of radius 4 at the far corner, final pass 0 then 1, as a
/// reveal trigger maps it.
fn reveal_between_frames(
    sim: &mut Simulation,
    rules: &RuleSet,
    frame: u32,
    viewer: InternedId,
    firer_owner: InternedId,
) {
    let config = sim.sight_reveal_config(Some(rules));
    let heights = sim.sight_height_grid(&config);
    if frame % 25 == 0
        && let Some(coord) = sim
            .substrate
            .entities
            .values()
            .find(|entity| entity.owner() == firer_owner)
            .map(|entity| crate::sim::movement::ground_pose::position_world_coord(&entity.position))
    {
        super::reveal_shroud_on_fire(&mut sim.fog, viewer, coord, &config, heights.levels());
    }
    if frame % 60 == 0 {
        for final_pass in [false, true] {
            let _ = sim.reveal_area(
                &[viewer],
                (28, 28),
                0,
                4,
                None,
                final_pass,
                &config,
                UninitContext::default(),
                crate::sim::vision::MapRevealProjection::Transient,
                sim.session.current_house,
            );
        }
    }
}

/// Tanks and infantrymen of two hostile Houses attack-move across the arena
/// into each other: sources move and die, and fire and a reveal trigger give
/// transient sight ([`reveal_between_frames`]). Every frame's state hash,
/// which folds every fog grid and sight admission, is the same whether the
/// frame passes visit the tracked cells or every cell and footprint.
#[test]
#[ignore = "slow; run when changing what it covers"]
fn tracked_fog_passes_match_visiting_every_cell() {
    let Some(rules) = retail_unlayered_rules() else {
        return;
    };
    let run = |every_cell: bool| {
        super::VISIT_EVERY_CELL.set(every_cell);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x70AF50);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.input_delay_ticks = 0;
        let grid = sim
            .path_grid_snapshot()
            .map(|grid| (*grid).clone())
            .expect("arena grid");
        let tick = sim.session.tick + 1;
        let mut orders = Vec::new();
        for (row, type_name) in ["MTNK", "E1", "HTNK", "E1", "MTNK", "E2"]
            .into_iter()
            .enumerate()
        {
            let y = 6 + 3 * row as u16;
            for (house, from, to) in [("Americans", 4, 27), ("Russians", 27, 4)] {
                let id = sim
                    .spawn_object_at_height(type_name, house, from, y, 0, 0, &rules)
                    .expect("spawns");
                orders.push(CommandEnvelope::new(
                    sim.interner.intern(house),
                    tick,
                    Command::AttackMove {
                        entity_id: id,
                        target: None,
                        destination: Some(TargetKind::Cell(to, y)),
                    },
                ));
            }
        }
        let (americans, russians) = (
            sim.interner.intern("Americans"),
            sim.interner.intern("Russians"),
        );
        let start = sim.substrate.entities.len();
        let hashes: Vec<u64> = (0..400)
            .map(|frame| {
                let commands = if frame == 0 { &orders[..] } else { &[] };
                sim.advance_tick(commands, Some(&rules), Some(&grid), None, 67);
                reveal_between_frames(&mut sim, &rules, frame, americans, russians);
                sim.state_hash()
            })
            .collect();
        super::VISIT_EVERY_CELL.set(false);
        (hashes, start - sim.substrate.entities.len())
    };
    let (tracked, deaths) = run(false);
    assert!(deaths > 0, "the armies fight");
    assert_eq!(tracked, run(true).0);
}
