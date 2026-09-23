//! Test-only flat arena: a clear 32x32 map with its playfield, zones and path
//! grid, for fixtures that order units around through `advance_tick`.

use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

/// Install the arena on `sim` and return its path grid.
pub(crate) fn flat_arena(sim: &mut Simulation, rules: &RuleSet) -> PathGrid {
    const SIZE: u16 = 32;
    sim.input_delay_ticks = 0;
    sim.session.map_width = SIZE;
    sim.session.map_height = SIZE;
    let clear = crate::rules::terrain_rules::SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: None,
        amphibious: Some(80),
        float_beach: None,
        hover: Some(50),
    };
    let cell = |x, y| {
        let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
        cell.speed_costs = clear;
        cell.base_speed_costs = clear;
        cell
    };
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            SIZE,
            SIZE,
            (0..SIZE)
                .flat_map(|y| (0..SIZE).map(move |x| cell(x, y)))
                .collect(),
        ),
    );
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 20,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    });
    sim.playfield_size_height = Some(20);
    assert!(sim.rebuild_dynamic_navigation(rules));
    sim.path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .expect("navigation grid")
}
