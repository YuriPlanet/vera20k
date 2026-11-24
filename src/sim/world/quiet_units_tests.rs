//! Quiet units change no result: a quiet tank and a twin with the switch off
//! stay in the same state, frame by frame.

use super::Simulation;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::scenario_session::ScenarioGameMode;

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\n[InfantryTypes]\n[VehicleTypes]\n0=MTNK\n[AircraftTypes]\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nROT=5\nTurret=yes\nSight=6\n",
    ))
    .expect("quiet unit test rules parse")
}

/// A skirmish with one tank on Guard (entity 1), quiet units on or off.
fn tank(rules: &RuleSet, quiet: bool) -> (Simulation, PathGrid) {
    let mut sim = Simulation::with_seed(0x51EE_9000_0002);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    let grid = crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 10,
            cell_y: 10,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: Some(0),
            high: false,
            mission: None,
            trigger_tag: None,
            recruitable_a: true,
            recruitable_b: true,
            group: -1,
            follows: None,
            structure_columns: Default::default(),
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
        }],
        Some(rules),
    );
    sim.sleep_idle_objects(quiet);
    (sim, grid)
}

fn tick(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
    let _ = sim.advance_tick(&[], Some(rules), Some(grid), None, 67);
}

/// The tank goes quiet once its first orders settle and stays so between
/// its mission and scan timers. A write that turns a skipped step on (its
/// turret turned away) wakes it, and the step runs on its next turn: the
/// turret swings home as in the twin, and the tank goes quiet again.
#[test]
fn a_quiet_tank_matches_its_twin_frame_by_frame() {
    let rules = rules();
    let (mut quiet, grid) = tank(&rules, true);
    let (mut loud, loud_grid) = tank(&rules, false);
    let mut quiet_turns = [0u32; 2];
    for frame in 0..300 {
        if frame == 150 {
            for sim in [&mut quiet, &mut loud] {
                let now = sim.session.binary_frame();
                let tank = sim.substrate.entities.get_mut(1).unwrap();
                tank.barrel_facing.as_mut().unwrap().set(0x8000, now);
            }
            assert!(!quiet.quiet(1), "a mutable hand-out ends the quiet");
        }
        tick(&mut quiet, &rules, &grid);
        tick(&mut loud, &rules, &loud_grid);
        assert_eq!(
            quiet.state_hash(),
            loud.state_hash(),
            "the twins differ after frame {frame}"
        );
        quiet_turns[usize::from(frame >= 150)] += u32::from(quiet.quiet(1));
        assert!(!loud.quiet(1), "the switch keeps the twin loud");
    }
    assert_eq!(
        serde_json::to_value(&quiet.substrate.entities).unwrap(),
        serde_json::to_value(&loud.substrate.entities).unwrap(),
    );
    assert!(
        quiet_turns[0] > 100 && quiet_turns[1] > 100,
        "frames the tank ended quiet, before and after the write: {quiet_turns:?}"
    );
}
