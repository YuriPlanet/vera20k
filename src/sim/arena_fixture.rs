//! Test-only flat arena: a clear 32x32 map with its playfield, zones and path
//! grid, for fixtures that order units around through `advance_tick`, and
//! the native map inputs a component fixture's Foot Process reads.

use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

/// Explicit rules prior for a synthetic clear-ground component scene.
/// Native LandType defaults are not buildable; an arena intended to host
/// ordinary buildings must supply the retail Clear `Buildable=yes` input.
/// Merge the scene last so its deliberate overrides still take precedence.
pub(crate) fn clear_ground_ini(source: &str) -> crate::rules::ini_parser::IniFile {
    let mut ini = crate::rules::ini_parser::IniFile::from_str("[Clear]\nBuildable=yes\n");
    ini.merge(&crate::rules::ini_parser::IniFile::from_str(source));
    ini
}

/// The arena's playfield, wide enough to hold every arena cell (and the shared
/// dummy just past its edge).
pub(crate) const OPEN_PLAYFIELD: crate::sim::cell_rect::PlayfieldBounds =
    crate::sim::cell_rect::PlayfieldBounds {
        base: 20,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    };

/// The arena's ground with two rival houses, not allied: `Russians`, a Normal
/// computer house of the skirmish IQ (`[IQ] MaxIQLevels`), and `Americans`, a
/// human house.
pub(crate) fn rival_arena(rules: &RuleSet, seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    flat_ground(&mut sim, rules);
    for (name, human) in [("Russians", false), ("Americans", true)] {
        let id = sim.interner.intern(name);
        let mut house = crate::sim::house_state::HouseState::new(id, 0, None, human, 0, 10);
        house.difficulty = crate::sim::house_state::HouseDifficulty::Normal;
        house.current_iq = if human {
            0
        } else {
            rules.general.max_iq_levels
        };
        sim.houses.insert(id, house);
        sim.session.house_order.push(id);
    }
    sim
}

/// Install the arena on `sim`, with commands executing on the next tick, and
/// return its path grid.
pub(crate) fn flat_arena(sim: &mut Simulation, rules: &RuleSet) -> PathGrid {
    sim.input_delay_ticks = 0;
    flat_ground(sim, rules)
}

/// Install the arena's ground, playfield and navigation on `sim` and return
/// its path grid.
pub(crate) fn flat_ground(sim: &mut Simulation, rules: &RuleSet) -> PathGrid {
    const SIZE: u16 = 32;
    sim.session.map_width = SIZE;
    sim.session.map_height = SIZE;
    sim.install_resolved_terrain_for_new_map(clear_ground(SIZE));
    sim.playfield_bounds = Some(OPEN_PLAYFIELD);
    sim.playfield_size_height = Some(20);
    assert!(sim.rebuild_dynamic_navigation(rules));
    sim.path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .expect("navigation grid")
}

/// A `size` x `size` grid of clear ground cells.
fn clear_ground(size: u16) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
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
    crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
        size,
        size,
        (0..size)
            .flat_map(|y| (0..size).map(move |x| cell(x, y)))
            .collect(),
    )
}

/// Supply whatever native map input a component fixture lacks for a Foot
/// Process that searches: map cells (clear 64x64 ground), zones and Map Size
/// for those cells, generous LocalSize bounds, and the path grid. Inputs the
/// fixture already installed are kept, including a zone grid built before a
/// later terrain change; rebuild zones after editing cells.
pub(crate) fn supply_native_map(sim: &mut Simulation) {
    if sim.resolved_terrain.is_none() {
        // A newly supplied map also owns its saved real CellClass values.
        // Keep already installed terrain and deliberate native priors intact.
        sim.install_resolved_terrain_for_new_map(clear_ground(64));
    }
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let (width, height) = (terrain.width(), terrain.height());
    let grid = PathGrid::from_resolved_terrain(terrain);
    let size = (i32::from(width), i32::from(height));
    if sim.zone_grid.is_none() {
        sim.zone_grid = Some(
            crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_bridge_geometry(
                &grid,
                terrain,
                &[],
                width,
                height,
                Some(size),
            ),
        );
    }
    if sim.playfield_bounds.is_none() {
        let span = size.0.max(size.1);
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: size.0,
            off_fc: -span,
            off_100: -span,
            off_104: span * 2,
            off_108: span * 2,
        });
    }
    sim.playfield_size_height.get_or_insert(size.1);
    sim.install_fixture_path_grid(Some(&grid));
}
