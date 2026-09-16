//! Retail-map high-bridge crossing check.
//!
//! The synthetic `PathGrid` fixtures elsewhere in this module answer "does the
//! resolver compute the right number". This module answers the different
//! question the bug report actually asks: on a **real retail map**, loaded
//! through the ordinary headless scenario funnel, does a ground vehicle ordered
//! across an intact high bridge stand on the deck for the whole span instead of
//! dropping to the riverbed underneath?
//!
//! The invariant asserted after every committed frame is the algebraic inverse
//! of `FootClass::Set_Height_On_Bridge` @ `0x005F5FA0` recorded by
//! `ObjectClass::GetHeight` @ `0x005F5F30`:
//!
//! ```text
//! position.z == GroundHeight(own cell) + (OnBridge ? 4 levels : 0)
//! ```
//!
//! Retail assets are not in the repo, so these are `#[ignore]`d and additionally
//! skip gracefully when no retail root resolves (the `RA2_DIR` → `config.toml`
//! order the rest of the tree uses). Run them with:
//!
//! ```text
//! cargo test -p vera20k --lib sim::movement::movement_bridge_retail_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use super::movement_occupancy::BRIDGE_DECK_LEVEL_DELTA;
use crate::headless_scenario::{self, SIM_TICK_MS};
use crate::map::resolved_terrain::{BridgeDirection, ResolvedTerrainGrid};
use crate::rules::locomotor_type::MovementZone;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;
use crate::sim::runtime::SimRuntime;
use crate::sim::world::TickLane;

/// Fixed seed so a failing run is reproducible.
const SEED: u32 = 0x0B21_D6E5;
/// Enough committed frames for a ~15-cell drive at stock tank speed.
const MAX_TICKS: u64 = 2000;

#[path = "bridge_restamp_retail_probe.rs"]
mod bridge_restamp_retail_probe;
#[path = "bridge_tile_retail_probe.rs"]
mod bridge_tile_retail_probe;

/// Retail install root, or `None` to skip.
fn retail_dir() -> Option<PathBuf> {
    let dir = match std::env::var("RA2_DIR") {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => match crate::util::config::GameConfig::load() {
            Ok(config) => config.paths.ra2_dir,
            Err(_) => return None,
        },
    };
    dir.is_dir().then_some(dir)
}

/// One intact high-bridge crossing discovered from live path-grid facts.
#[derive(Debug, Clone)]
struct HighBridgeSpan {
    /// Off-bridge cell the drive starts on, one step before the first deck cell.
    approach_a: (u16, u16),
    /// Off-bridge cell on the far side, one step past the last deck cell.
    approach_b: (u16, u16),
    /// Every structural deck cell between them, in travel order.
    deck: Vec<(u16, u16)>,
    /// Terrain level *under* the deck (the riverbed the tank must never sit on).
    deck_terrain_level: u8,
    /// Terrain level of both approach cells; equals `deck_terrain_level + 4`.
    approach_level: u8,
    step: (i32, i32),
}

const STEPS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

fn offset(cell: (u16, u16), step: (i32, i32)) -> Option<(u16, u16)> {
    let x = i32::from(cell.0) + step.0;
    let y = i32::from(cell.1) + step.1;
    (x >= 0 && y >= 0).then(|| (x as u16, y as u16))
}

/// Find the longest straight run of structural deck cells that is entered and
/// left through cells exactly four levels above the deck's own terrain — i.e.
/// the geometry the `Enter`/`Exit` predicate (`0x004B2561`-`0x004B259B`) fires on.
fn find_high_bridge_span(grid: &PathGrid) -> Option<HighBridgeSpan> {
    let mut best: Option<HighBridgeSpan> = None;
    for y in 0..grid.height() {
        for x in 0..grid.width() {
            let Some(first) = grid.cell(x, y) else {
                continue;
            };
            if !first.bridge_structural {
                continue;
            }
            let deck_terrain_level = first.ground_level;
            let Some(approach_level) = deck_terrain_level.checked_add(4) else {
                continue;
            };
            for step in STEPS {
                // The cell behind the first deck cell must be a real approach:
                // off-bridge, walkable, and exactly four levels up.
                let back = (-step.0, -step.1);
                let Some(approach_a) = offset((x, y), back) else {
                    continue;
                };
                let Some(cell_a) = grid.cell(approach_a.0, approach_a.1) else {
                    continue;
                };
                if cell_a.bridge_structural
                    || !cell_a.ground_walkable
                    || cell_a.ground_level != approach_level
                {
                    continue;
                }
                let mut deck = vec![(x, y)];
                let mut cursor = (x, y);
                loop {
                    let Some(next) = offset(cursor, step) else {
                        break;
                    };
                    let Some(cell) = grid.cell(next.0, next.1) else {
                        break;
                    };
                    if !cell.bridge_structural || cell.ground_level != deck_terrain_level {
                        break;
                    }
                    deck.push(next);
                    cursor = next;
                }
                let Some(approach_b) = offset(cursor, step) else {
                    continue;
                };
                let Some(cell_b) = grid.cell(approach_b.0, approach_b.1) else {
                    continue;
                };
                if cell_b.bridge_structural
                    || !cell_b.ground_walkable
                    || cell_b.ground_level != approach_level
                {
                    continue;
                }
                let candidate = HighBridgeSpan {
                    approach_a,
                    approach_b,
                    deck_terrain_level,
                    approach_level,
                    step,
                    deck,
                };
                if best
                    .as_ref()
                    .is_none_or(|current| candidate.deck.len() > current.deck.len())
                {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

/// One committed frame of observed mover state.
#[derive(Debug, Clone, Copy)]
struct TickRow {
    tick: u64,
    cell: (u16, u16),
    z: u8,
    on_bridge: bool,
    occupancy_deck: Option<u8>,
    terrain_level: u8,
    structural: bool,
    bridge_walkable: bool,
    /// `PathCell::transition` — the `0x200` bridgehead flag
    /// (`BRIDGE_FLAG_TRANSITION`, `map/bridge_facts.rs`). Recorded so matrix rows
    /// T2-05/T2-06 ("ramp / bridgehead, onto") are asserted rather than assumed
    /// to ride along on a crossing: `stamp_intact` writes `0x200` on the Anchor,
    /// Forward1 and Opposite slots, so on both fixtures the whole drive line
    /// carries it — every deck step on these crossings *is* a bridgehead step.
    transition: bool,
    /// `PathCell::low_bridge_tube_cell` — the `TubeClass` low-span marker
    /// (`tube_index` present **and** final CellClass LandType 10). Recorded on
    /// both drivers so a high-span run states the two markers are disjoint
    /// rather than leaving it assumed.
    low_tube: bool,
    stored_deck_level: u8,
    /// The mover's live A* layer — the input the pre-fix height path keyed off.
    loco_layer: crate::sim::movement::locomotor::MovementLayer,
    /// What the pre-fix code would have written into `position.z` on this frame:
    /// `dst_cell.effective_cell_z_for_layer(layer)`, i.e. the stored per-cell deck
    /// value read through the `bridge_walkable` Option with an `unwrap_or(ground)`
    /// fallback. Recorded so a passing run says whether it actually discriminates
    /// the fix or merely fails to contradict it.
    prefix_z: u8,
}

impl TickRow {
    /// `ObjectClass::GetHeight` @ `0x005F5F30` inverted: the only Z the native
    /// model can produce for this cell and this OnBridge state.
    fn expected_z(&self) -> i16 {
        i16::from(self.terrain_level as i8)
            + if self.on_bridge {
                BRIDGE_DECK_LEVEL_DELTA
            } else {
                0
            }
    }

    fn holds_invariant(&self) -> bool {
        i16::from(self.z as i8) == self.expected_z()
    }
}

fn print_inventory(grid: &PathGrid, span: &HighBridgeSpan) {
    let structural = (0..grid.height())
        .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
        .filter(|(x, y)| grid.cell(*x, *y).is_some_and(|c| c.bridge_structural))
        .count();
    println!(
        "path grid {}x{}, {structural} structural high-bridge cell(s)",
        grid.width(),
        grid.height()
    );
    println!(
        "span: approach_a {:?} (level {}) -> {} deck cell(s) at terrain level {} -> approach_b {:?} (level {}), step {:?}",
        span.approach_a,
        span.approach_level,
        span.deck.len(),
        span.deck_terrain_level,
        span.approach_b,
        span.approach_level,
        span.step,
    );
    // `ground_walkable` is the one that settles matrix evidence gap 1: whether
    // the terrain *under* a high span is passable anywhere in the corpus. Three
    // rows (T1-13/14/15, "under" for each locomotor) are demotion-pending on it —
    // if no retail map has walkable ground beneath a deck, there is nothing to
    // drive under and those rows are NOT-APPLICABLE rather than untested.
    let mut walkable_under = 0usize;
    for cell in &span.deck {
        let facts = grid.cell(cell.0, cell.1).expect("deck cell in bounds");
        if facts.ground_walkable {
            walkable_under += 1;
        }
        println!(
            "  deck {:?}: ground_level={} bridge_deck_level={} structural={} walkable={} transition={} ground_walkable={}",
            cell,
            facts.ground_level,
            facts.bridge_deck_level,
            facts.bridge_structural,
            facts.bridge_walkable,
            facts.transition,
            facts.ground_walkable,
        );
    }
    println!(
        "  UNDER-SPAN: {}/{} deck cells report ground_walkable — {}",
        walkable_under,
        span.deck.len(),
        if walkable_under == 0 {
            "nothing can drive beneath this span"
        } else {
            "an under-span route exists here (matrix gap 1 fixture)"
        }
    );
}

/// The per-frame observation table both crossing drivers print.
fn print_tick_table(rows: &[TickRow]) {
    println!(
        "\ntick  cell        z  on_bridge  occ_deck  terrain  deck?  walkable  lowtube  stored_deck  layer   prefix_z  expect_z  ok"
    );
    for row in rows {
        println!(
            "{:5} ({:3},{:3}) {:3}  {:9}  {:8}  {:7}  {:5}  {:8}  {:7}  {:11}  {:6}  {:8}  {:8}  {}",
            row.tick,
            row.cell.0,
            row.cell.1,
            row.z,
            row.on_bridge,
            row.occupancy_deck
                .map(|d| d.to_string())
                .unwrap_or_else(|| "-".to_string()),
            row.terrain_level,
            row.structural,
            row.bridge_walkable,
            row.low_tube,
            row.stored_deck_level,
            format!("{:?}", row.loco_layer),
            row.prefix_z,
            row.expected_z(),
            if row.holds_invariant() { "ok" } else { "FAIL" },
        );
    }
}

/// Give the loaded scenario a commandable house and return its name.
///
/// A headless load is a spectatorless map load, so the roster may be empty; the
/// mover needs an owner whose name the order path can match. Every house is made
/// passive for the run: the defeat scan would otherwise resolve the match on the
/// first frame and stop committing ticks.
fn prepare_commanding_house(scenario: &mut crate::headless_scenario::HeadlessScenario) -> String {
    let sim = &mut scenario.runtime.simulation;
    let existing: Vec<String> = sim
        .houses
        .keys()
        .map(|id| sim.interner.resolve(*id).to_string())
        .collect();
    println!("map roster houses: {existing:?}");
    let name = existing
        .iter()
        .find(|name| !name.eq_ignore_ascii_case("Neutral") && !name.eq_ignore_ascii_case("Special"))
        .cloned()
        .unwrap_or_else(|| {
            let name = "Americans".to_string();
            let id = sim.interner.intern(&name);
            sim.houses
                .insert(id, HouseState::new(id, 0, None, true, 10_000, 10));
            name
        });
    for house in sim.houses.values_mut() {
        house.multiplay_passive = true;
    }
    let id = sim.interner.get(&name).expect("owner interned");
    if sim.session.house_order.is_empty() {
        sim.session.house_order = vec![id];
    }
    name
}

/// Report why the order never became a `MovementTarget`, so a harness failure is
/// distinguishable from a height defect.
///
/// Takes the span geometry as loose cells rather than a `HighBridgeSpan` so the
/// low-span driver gets the same diagnosis — a refusal is exactly as plausible
/// there, and it has never been measured.
fn diagnose_rejected_order(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    owner_name: &str,
    entity_id: u64,
    start_cell: (u16, u16),
    approach_a: (u16, u16),
    approach_b: (u16, u16),
    deck: &[(u16, u16)],
) {
    use crate::sim::pathfinding::{AStarOptions, astar_search, find_path};

    /// Borrowed view so the ablation body below reads identically for a high
    /// span (`HighBridgeSpan`) and a low one (`LowBridgeSpan`).
    struct SpanView<'a> {
        approach_a: (u16, u16),
        approach_b: (u16, u16),
        deck: &'a [(u16, u16)],
    }
    let span = SpanView {
        approach_a,
        approach_b,
        deck,
    };

    let grid = scenario
        .sim()
        .path_grid_snapshot()
        .expect("navigation published");
    println!("--- order rejected; diagnosing ---");
    println!(
        "owner match: {}",
        scenario.sim().entity_owned_by_id(owner_name, entity_id)
    );
    println!(
        "order_actor_admits: {}",
        scenario.sim().order_actor_admits(entity_id)
    );
    println!(
        "flat A* {start_cell:?} -> {:?}: {:?} node(s)",
        span.approach_b,
        find_path(&grid, start_cell, span.approach_b).map(|path| path.len())
    );
    let layered = astar_search(
        &grid,
        start_cell,
        crate::sim::movement::locomotor::MovementLayer::Ground,
        span.approach_b,
        &AStarOptions::default(),
    );
    match &layered {
        Some(steps) => {
            println!("layered A*: {} step(s)", steps.len());
            for step in steps.iter().take(30) {
                println!("   ({:3},{:3}) layer={:?}", step.rx, step.ry, step.layer);
            }
        }
        None => println!("layered A*: no path"),
    }

    // Ablation over the four production inputs the bare `astar_search` above did
    // not carry, to name which one refuses a route the raw search finds.
    {
        use super::PathfindingContext;
        use super::movement_path::find_move_path;
        use crate::rules::locomotor_type::MovementZone;
        use crate::sim::movement::locomotor::MovementLayer;

        let sim = scenario.sim();
        let entity = sim.entities().get(entity_id).expect("tank present");
        let speed_type = entity.locomotor.as_ref().map(|loco| loco.speed_type);
        let movement_zone = entity.locomotor.as_ref().map(|loco| loco.movement_zone);
        println!(
            "in_playfield={} speed_type={:?} movement_zone={:?}",
            entity.in_playfield, speed_type, movement_zone
        );
        let costs = speed_type.and_then(|st| sim.terrain_costs.get(&st));
        println!(
            "terrain cost grid for {speed_type:?}: {}, zone_grid: {}, resolved_terrain: {}, playfield_bounds: {:?}",
            costs.is_some(),
            sim.zone_grid.is_some(),
            sim.resolved_terrain.is_some(),
            sim.playfield_bounds.is_some(),
        );
        let (blocks, block_map) = crate::sim::movement::bump_crush::build_entity_block_set(
            sim.entities(),
            owner_name,
            &sim.house_alliances,
            &sim.interner,
            Some(&scenario.runtime.resources.rules),
        );
        println!("entity blocks: {} cell(s)", blocks.len());
        let on_span: Vec<&(u16, u16)> = blocks
            .iter()
            .filter(|cell| span.deck.contains(cell) || **cell == span.approach_b)
            .collect();
        println!("  blocks on the span or its far approach: {on_span:?}");

        // The two production inputs the plain block set does not carry: the live
        // vehicle occupation plane, folded into the same set, and the blocker
        // neighbour counts the edge-cost walk reads.
        let mut occupation_blocks = blocks.clone();
        occupation_blocks.extend(
            scenario
                .sim()
                .substrate
                .cell_occupation
                .occupied_cells_ignoring(MovementLayer::Ground, entity_id),
        );
        println!(
            "entity blocks + ground vehicle occupation: {} cell(s); on span: {:?}",
            occupation_blocks.len(),
            occupation_blocks
                .iter()
                .filter(|cell| span.deck.contains(cell)
                    || **cell == span.approach_b
                    || **cell == span.approach_a)
                .collect::<Vec<_>>()
        );
        let neighbor_counts =
            crate::sim::movement::bump_crush::build_blocker_neighbor_counts_with_overlays(
                sim.entities(),
                grid.width(),
                grid.height(),
                sim.resolved_terrain.as_ref(),
                sim.overlay_grid.as_ref(),
                Some(&scenario.runtime.resources.overlay_registry),
                &sim.interner,
                Some(&scenario.runtime.resources.rules),
            );

        for (label, use_zone, use_costs, use_terrain, use_bounds, use_blocks) in [
            ("bare", false, false, false, false, false),
            ("+zone", true, false, false, false, false),
            ("+costs", false, true, false, false, false),
            ("+terrain", false, false, true, false, false),
            ("+bounds", false, false, false, true, false),
            ("+blocks", false, false, false, false, true),
            ("all", true, true, true, true, true),
        ] {
            let ctx = PathfindingContext {
                wall_tables: None,
                path_grid: Some(&grid),
                zone_grid: if use_zone {
                    sim.zone_grid.as_ref()
                } else {
                    None
                },
                resolved_terrain: if use_terrain {
                    sim.resolved_terrain.as_ref()
                } else {
                    None
                },
                playfield_bounds: if use_bounds {
                    sim.playfield_bounds
                } else {
                    None
                },
                blocker_neighbor_counts: None,
            };
            let block_ref = use_blocks.then_some(&blocks);
            let result = find_move_path(
                ctx,
                true,
                start_cell,
                MovementLayer::Ground,
                span.approach_b,
                if use_costs { costs } else { None },
                block_ref,
                block_ref,
                block_ref,
                movement_zone.unwrap_or(MovementZone::Normal),
                movement_zone,
                false,
                use_blocks.then_some(&block_map),
                0,
                false,
                false,
                true,
            );
            println!(
                "  find_move_path[{label}] -> {:?}",
                result.map(|(path, _)| path.len())
            );
        }

        // Exact production argument set.
        let production = find_move_path(
            PathfindingContext {
                wall_tables: None,
                path_grid: Some(&grid),
                zone_grid: sim.zone_grid.as_ref(),
                resolved_terrain: sim.resolved_terrain.as_ref(),
                playfield_bounds: sim.playfield_bounds,
                blocker_neighbor_counts: Some(&neighbor_counts),
            },
            true,
            start_cell,
            MovementLayer::Ground,
            span.approach_b,
            costs,
            Some(&occupation_blocks),
            Some(&occupation_blocks),
            Some(&occupation_blocks),
            movement_zone.unwrap_or(MovementZone::Normal),
            movement_zone,
            false,
            Some(&block_map),
            0,
            true,
            false,
            true,
        );
        println!(
            "  find_move_path[production] -> {:?}",
            production.as_ref().map(|(path, _)| path.len())
        );

        // Same, minus one input at a time.
        let without_occupation = find_move_path(
            PathfindingContext {
                wall_tables: None,
                path_grid: Some(&grid),
                zone_grid: sim.zone_grid.as_ref(),
                resolved_terrain: sim.resolved_terrain.as_ref(),
                playfield_bounds: sim.playfield_bounds,
                blocker_neighbor_counts: Some(&neighbor_counts),
            },
            true,
            start_cell,
            MovementLayer::Ground,
            span.approach_b,
            costs,
            Some(&blocks),
            Some(&blocks),
            Some(&blocks),
            movement_zone.unwrap_or(MovementZone::Normal),
            movement_zone,
            false,
            Some(&block_map),
            0,
            true,
            false,
            true,
        );
        println!(
            "  find_move_path[production - occupation] -> {:?}",
            without_occupation.as_ref().map(|(path, _)| path.len())
        );
        let without_neighbors = find_move_path(
            PathfindingContext {
                wall_tables: None,
                path_grid: Some(&grid),
                zone_grid: sim.zone_grid.as_ref(),
                resolved_terrain: sim.resolved_terrain.as_ref(),
                playfield_bounds: sim.playfield_bounds,
                blocker_neighbor_counts: None,
            },
            true,
            start_cell,
            MovementLayer::Ground,
            span.approach_b,
            costs,
            Some(&occupation_blocks),
            Some(&occupation_blocks),
            Some(&occupation_blocks),
            movement_zone.unwrap_or(MovementZone::Normal),
            movement_zone,
            false,
            Some(&block_map),
            0,
            true,
            false,
            true,
        );
        println!(
            "  find_move_path[production - neighbors] -> {:?}",
            without_neighbors.as_ref().map(|(path, _)| path.len())
        );
        let without_block_map = find_move_path(
            PathfindingContext {
                wall_tables: None,
                path_grid: Some(&grid),
                zone_grid: sim.zone_grid.as_ref(),
                resolved_terrain: sim.resolved_terrain.as_ref(),
                playfield_bounds: sim.playfield_bounds,
                blocker_neighbor_counts: Some(&neighbor_counts),
            },
            true,
            start_cell,
            MovementLayer::Ground,
            span.approach_b,
            costs,
            Some(&occupation_blocks),
            Some(&occupation_blocks),
            Some(&occupation_blocks),
            movement_zone.unwrap_or(MovementZone::Normal),
            movement_zone,
            false,
            None,
            0,
            true,
            false,
            true,
        );
        println!(
            "  find_move_path[production - block_map] -> {:?}",
            without_block_map.as_ref().map(|(path, _)| path.len())
        );

        if let Some(zones) = sim
            .zone_grid
            .as_ref()
            .and_then(|grid| grid.map_for(movement_zone.unwrap_or(MovementZone::Normal)))
        {
            let show = |cell: (u16, u16)| {
                format!(
                    "{cell:?} ground={:?} bridge={:?}",
                    zones.zone_at(cell.0, cell.1, MovementLayer::Ground),
                    zones.zone_at(cell.0, cell.1, MovementLayer::Bridge)
                )
            };
            println!("  zones: approach_a {}", show(span.approach_a));
            for cell in span.deck.iter().take(4) {
                println!("         deck       {}", show(*cell));
            }
            println!("         approach_b {}", show(span.approach_b));
        }

        // How far along the span does the production search get before the gate
        // shuts? Read-only: nothing here mutates the simulation.
        println!("  production-config search to successive goals along the span:");
        let mut goals: Vec<(u16, u16)> = vec![span.approach_a];
        goals.extend(span.deck.iter().copied());
        goals.push(span.approach_b);
        for goal in goals {
            let reached = find_move_path(
                PathfindingContext {
                    wall_tables: None,
                    path_grid: Some(&grid),
                    zone_grid: sim.zone_grid.as_ref(),
                    resolved_terrain: sim.resolved_terrain.as_ref(),
                    playfield_bounds: sim.playfield_bounds,
                    blocker_neighbor_counts: Some(&neighbor_counts),
                },
                true,
                start_cell,
                MovementLayer::Ground,
                goal,
                costs,
                Some(&occupation_blocks),
                Some(&occupation_blocks),
                Some(&occupation_blocks),
                movement_zone.unwrap_or(MovementZone::Normal),
                movement_zone,
                false,
                Some(&block_map),
                0,
                true,
                false,
                true,
            );
            println!(
                "    -> {goal:?}: {:?}",
                reached.as_ref().map(|(path, _)| path.len())
            );
        }
    }

    let SimRuntime {
        simulation,
        resources,
    } = &mut scenario.runtime;
    println!(
        "resolve_move_info: {:?}",
        simulation
            .resolve_move_info(entity_id, Some(&resources.rules))
            .is_some()
    );
    let applied = simulation.apply_command(
        owner_name,
        &Command::Move {
            entity_id,
            target_rx: span.approach_b.0,
            target_ry: span.approach_b.1,
            queue: false,
            group_id: None,
        },
        Some(&resources.rules),
        Some(&grid),
        &resources.height_map,
    );
    println!("direct apply_command(Move) -> {applied}");
    println!(
        "movement_target after direct apply: {:?}",
        simulation
            .entities()
            .get(entity_id)
            .and_then(|entity| entity.movement_target.as_ref())
            .map(|target| target.path.len())
    );
}

/// Issue an ordinary player `Command::Move` — the same entry point a right-click
/// goes through, with nothing disabled.
///
/// Every order this harness issues, the crossing and the control alike, goes
/// through here. A control move that used a widened search could pass while the
/// crossing was refused by a gate, which would report "the mover is fine, the
/// bridge is the problem" from two different code paths and prove neither.
fn issue_ordinary_move(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    owner_name: &str,
    entity_id: u64,
    target: (u16, u16),
) -> bool {
    let grid = scenario
        .sim()
        .path_grid_snapshot()
        .expect("navigation published");
    let SimRuntime {
        simulation,
        resources,
    } = &mut scenario.runtime;
    simulation.apply_command(
        owner_name,
        &Command::Move {
            entity_id,
            target_rx: target.0,
            target_ry: target.1,
            queue: false,
            group_id: None,
        },
        Some(&resources.rules),
        Some(&grid),
        &resources.height_map,
    )
}

/// Which player order carries the crossing.
///
/// Matrix rows T2-01/T2-04 exist because "no attack-move across a span has ever
/// been run by any locomotor". Both variants are ordinary undisabled orders a
/// player issues by right-click / A-click; nothing here is a test-only entry
/// point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderSource {
    /// `Command::Move` — the plain right-click.
    Move,
    /// `Command::AttackMove` — the A-click. Its *move* half calls
    /// `movement::issue_move_command_with_layered` with an argument list
    /// identical to `Command::Move`'s (`world_commands.rs`, the `Move` arm and
    /// the `AttackMove` arm), so what this variant adds over `Move` is the
    /// surrounding order machinery: the `MissionType::AttackMove` megamission,
    /// the `attack_target`/`passively_acquired_target` clear, the
    /// `OrderIntent::AttackMove` stamp, and the per-tick resume in
    /// `tick_order_intents_post_combat_*` that re-issues the move whenever
    /// combat interrupts it. That resume is the part a plain Move never
    /// exercises, and it is the part that can strand a unit at a bridgehead.
    AttackMove,
}

impl OrderSource {
    fn command(self, entity_id: u64, target: (u16, u16)) -> Command {
        match self {
            OrderSource::Move => Command::Move {
                entity_id,
                target_rx: target.0,
                target_ry: target.1,
                queue: false,
                group_id: None,
            },
            OrderSource::AttackMove => Command::AttackMove {
                entity_id,
                target_rx: target.0,
                target_ry: target.1,
                queue: false,
            },
        }
    }
}

/// Load a retail map, spawn a tank on one bridge approach, order it to the far
/// approach, and record `position.z` against the native height invariant after
/// every committed frame.
fn drive_across_high_bridge(map_file: &str, unit_type: &str, expect_crossing: bool) {
    drive_across_high_bridge_with_order(map_file, unit_type, expect_crossing, OrderSource::Move);
}

fn drive_across_high_bridge_with_order(
    map_file: &str,
    unit_type: &str,
    expect_crossing: bool,
    order_source: OrderSource,
) {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    // The move-issue path reports every refusal reason through `log::warn!`; without
    // a logger a rejected order is indistinguishable from a height defect.
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let mut scenario = match headless_scenario::load(&retail, map_file, SEED) {
        Ok(scenario) => scenario,
        Err(error) => panic!("load {map_file}: {error}"),
    };
    println!(
        "loaded {map_file} ({}x{}, theater {})",
        scenario.sim().session.map_width,
        scenario.sim().session.map_height,
        scenario.map.header.theater,
    );

    let span = {
        let grid = scenario
            .sim()
            .path_grid()
            .expect("headless load publishes navigation");
        let Some(span) = find_high_bridge_span(grid) else {
            panic!(
                "{map_file} exposes no high-bridge span: no structural deck run is entered and \
                 left through cells exactly four levels above it"
            );
        };
        print_inventory(grid, &span);
        span
    };
    assert!(
        span.deck.len() >= 3,
        "a two-cell span proves nothing about mid-span behaviour; found {} deck cell(s)",
        span.deck.len()
    );

    let owner_name = prepare_commanding_house(&mut scenario);
    println!("commanding house: {owner_name}");

    // Spawn on the near approach; fall back one cell further back if that cell
    // is already occupied by map-placed content.
    let start_candidates = [
        span.approach_a,
        offset(span.approach_a, (-span.step.0, -span.step.1)).unwrap_or(span.approach_a),
    ];
    let mut entity_id = None;
    let mut start_cell = span.approach_a;
    for candidate in start_candidates {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        if let Some(id) = simulation.spawn_object(
            unit_type,
            &owner_name,
            candidate.0,
            candidate.1,
            0,
            &resources.rules,
            &resources.height_map,
        ) {
            entity_id = Some(id);
            start_cell = candidate;
            break;
        }
    }
    let entity_id = entity_id.unwrap_or_else(|| {
        panic!("could not place a {unit_type} on either approach cell {start_candidates:?}")
    });
    // The headless funnel does not build the type-handle table (nothing it loads
    // reaches combat); an armed vehicle does, and combat asserts on it.
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    {
        let entity = scenario
            .sim()
            .entities()
            .get(entity_id)
            .expect("spawned tank present");
        println!(
            "spawned {unit_type} id={entity_id} at {start_cell:?} z={} on_bridge={}",
            entity.position.z, entity.on_bridge
        );
    }

    // Order the crossing.
    let owner_id = scenario
        .sim()
        .interner
        .get(&owner_name)
        .expect("owner interned");
    let execute_tick = scenario.sim().session.tick + 1;
    println!("order source: {order_source:?} -> {:?}", span.approach_b);
    let order = CommandEnvelope::new(
        owner_id,
        execute_tick,
        order_source.command(entity_id, span.approach_b),
    );
    scenario
        .runtime
        .advance_frame(&[order], SIM_TICK_MS, TickLane::Ordinary)
        .expect("fixture frame must complete");

    let ordered_path = scenario
        .sim()
        .entities()
        .get(entity_id)
        .and_then(|entity| entity.movement_target.as_ref())
        .map(|target| target.path.clone());
    match &ordered_path {
        Some(path) => println!(
            "move order accepted: {} node(s) {:?} -> {:?}",
            path.len(),
            path.first(),
            path.last()
        ),
        None => {
            // RATCHET. This branch used to re-issue the order with the
            // zone-hierarchy gate switched off, so a deck-height observation was
            // still possible while the order path itself stayed broken. That
            // fallback is deleted along with the defect it worked around
            // (cf91caa3): an ordinary, undisabled `Command::Move` is the only
            // route a player has, so it must succeed here or the run fails. A
            // harness that can route around a regression is not a ratchet.
            diagnose_rejected_order(
                &mut scenario,
                &owner_name,
                entity_id,
                start_cell,
                span.approach_a,
                span.approach_b,
                &span.deck,
            );
            panic!(
                "the ordinary Command::Move to {:?} was REFUSED — a player cannot cross this \
                 span at all. See the diagnosis above; the bridge-deck exemption from the \
                 zone-hierarchy corridor gate (sim::pathfinding::core) is the first thing to \
                 check.",
                span.approach_b
            );
        }
    }

    // Record every committed frame.
    let mut rows: Vec<TickRow> = Vec::new();
    let mut idle_frames = 0u32;
    for _ in 0..MAX_TICKS {
        scenario.tick();
        let sim = scenario.sim();
        let Some(entity) = sim.entities().get(entity_id) else {
            panic!("the tank vanished mid-crossing");
        };
        let cell = (entity.position.rx, entity.position.ry);
        let facts = sim
            .path_grid()
            .and_then(|grid| grid.cell(cell.0, cell.1))
            .copied();
        let Some(facts) = facts else {
            panic!("tank left the path grid at {cell:?}");
        };
        let loco_layer = entity.movement_layer_or_ground();
        rows.push(TickRow {
            tick: sim.session.tick,
            cell,
            z: entity.position.z,
            on_bridge: entity.on_bridge,
            occupancy_deck: entity.bridge_occupancy.map(|occ| occ.deck_level),
            terrain_level: facts.ground_level,
            structural: facts.bridge_structural,
            bridge_walkable: facts.bridge_walkable,
            transition: facts.transition,
            low_tube: facts.low_bridge_tube_cell,
            stored_deck_level: facts.bridge_deck_level,
            loco_layer,
            prefix_z: facts.effective_cell_z_for_layer(loco_layer),
        });
        if entity.movement_target.is_none() {
            idle_frames += 1;
            if idle_frames >= 30 {
                break;
            }
        } else {
            idle_frames = 0;
        }
        if cell == span.approach_b {
            break;
        }
    }

    print_tick_table(&rows);

    let last = rows.last().copied().expect("at least one frame observed");
    println!(
        "\n{} frame(s) recorded, last cell {:?} (target {:?})",
        rows.len(),
        last.cell,
        span.approach_b
    );

    // 1. The drive must actually have used the deck.
    let deck_frames: Vec<&TickRow> = rows.iter().filter(|row| row.structural).collect();
    if deck_frames.is_empty() {
        // Control: can this unit move at all, away from the bridge? Separates
        // "the mover is broken" from "the bridge route is refused".
        let control_goal = offset(start_cell, (-span.step.0 * 3, -span.step.1 * 3))
            .unwrap_or((start_cell.0, start_cell.1));
        let issued = issue_ordinary_move(&mut scenario, &owner_name, entity_id, control_goal);
        println!("control move away from the bridge to {control_goal:?} issued -> {issued}");
        let mut control_cells = Vec::new();
        for _ in 0..300 {
            scenario.tick();
            if let Some(entity) = scenario.sim().entities().get(entity_id) {
                let cell = (entity.position.rx, entity.position.ry);
                if control_cells.last() != Some(&cell) {
                    control_cells.push(cell);
                }
            }
        }
        println!("control move visited: {control_cells:?}");
        if !expect_crossing {
            // Characterization, NOT desired behaviour. This mover accepts the
            // order, never leaves its cell, and then drops the order — while the
            // same mover crosses ordinary ground in the control above. Recorded
            // as an assertion so the day the block is lifted this test goes red
            // and gets rewritten into a real crossing.
            assert!(
                control_cells.len() > 1,
                "the control move away from the bridge also failed, so this says nothing                  specific about the bridge: {control_cells:?}"
            );
            println!(
                "DEFECT CHARACTERIZED: {unit_type} accepted an order onto the span and never                  left {start_cell:?} in {} frames, yet moved {} cell(s) on ordinary ground in                  the control. The height invariant held on every observed frame, but no deck                  frame exists, so this run contributes nothing to the deck-height question.",
                rows.len(),
                control_cells.len() - 1,
            );
            let violations: Vec<&TickRow> =
                rows.iter().filter(|row| !row.holds_invariant()).collect();
            assert!(
                violations.is_empty(),
                "off-bridge height invariant broke: {violations:?}"
            );
            return;
        }
    }
    assert!(
        !deck_frames.is_empty(),
        "the tank never stood on a structural deck cell — it did not cross the bridge, \
         so this run says nothing about deck height. Cells visited: {:?}",
        rows.iter().map(|row| row.cell).collect::<Vec<_>>()
    );
    let visited_deck: std::collections::BTreeSet<(u16, u16)> =
        deck_frames.iter().map(|row| row.cell).collect();
    println!(
        "stood on {} of {} deck cell(s): {:?}",
        visited_deck.len(),
        span.deck.len(),
        visited_deck
    );

    // 2. The native height invariant, every frame, ramps included.
    let violations: Vec<&TickRow> = rows.iter().filter(|row| !row.holds_invariant()).collect();
    assert!(
        violations.is_empty(),
        "position.z left the native model on {} frame(s); first: {:?} (expected z {}, got {})",
        violations.len(),
        violations[0],
        violations[0].expected_z(),
        violations[0].z,
    );

    // 3. On the deck the tank is on the deck: flagged on-bridge, at deck height,
    //    never at the riverbed level under the span.
    for row in &deck_frames {
        assert!(
            row.on_bridge,
            "on structural deck cell {:?} the tank was not marked on_bridge: {row:?}",
            row.cell
        );
        assert_eq!(
            i16::from(row.z as i8),
            i16::from(row.terrain_level as i8) + BRIDGE_DECK_LEVEL_DELTA,
            "tank is not at deck height on {:?}: {row:?}",
            row.cell
        );
        assert_ne!(
            row.z, span.deck_terrain_level,
            "tank dropped to the terrain under the bridge at {:?}: {row:?}",
            row.cell
        );
        assert_eq!(
            row.occupancy_deck,
            Some(row.z),
            "BridgeOccupancy.deck_level disagrees with position.z at {:?}: {row:?}",
            row.cell
        );
    }

    // 3b. Matrix rows T2-05 / T2-06 — ramp / bridgehead, "onto".
    //
    // A bridgehead cell is one carrying `BRIDGE_FLAG_TRANSITION` (`0x200`),
    // written by `stamp_intact` on the Anchor, Forward1 and Opposite slots
    // (`map/bridge_facts.rs`) and surfaced as `PathCell::transition`. It is the
    // cell `cell_entry.rs`'s `evaluate_shared_cell_leaf` short-circuits on,
    // returning Clear/HardBlocked from `land_passable` alone and skipping the
    // object-list and speed-row half of the entry test entirely.
    //
    // Asserting it here, rather than treating it as riding along on the deck
    // frames, is the same discipline T1-05 and T1-07 were held to: a crossing
    // that happens to pass over a bridgehead does not settle a bridgehead row
    // unless it says so. Measured consequence, and it is bigger than the row's
    // wording implies — on both fixtures **every** deck cell of the drive line
    // carries `0x200`, not just the two end ramps, because consecutive anchors
    // each stamp their own Anchor/F1/Opposite neighbourhood along the line. So
    // the short-circuit is the entry rule for the entire span, and each crossing
    // steps onto a bridgehead cell 17 (BayOPigs) or 22 (Hills) times.
    let bridgehead_frames: Vec<&TickRow> = deck_frames
        .iter()
        .filter(|row| row.transition)
        .copied()
        .collect();
    let bridgehead_cells: std::collections::BTreeSet<(u16, u16)> =
        bridgehead_frames.iter().map(|row| row.cell).collect();
    assert!(
        !bridgehead_frames.is_empty(),
        "no frame stood on a `0x200` bridgehead/transition cell, so this run settles nothing \
         about the ramp rows; deck cells visited: {visited_deck:?}"
    );
    for row in &bridgehead_frames {
        assert!(
            row.on_bridge,
            "on bridgehead cell {:?} the mover was not marked on_bridge: {row:?}",
            row.cell
        );
        assert_eq!(
            i16::from(row.z as i8),
            i16::from(row.terrain_level as i8) + BRIDGE_DECK_LEVEL_DELTA,
            "the mover is not at deck height on bridgehead cell {:?}: {row:?}",
            row.cell
        );
    }
    println!(
        "BRIDGEHEAD (0x200): {} frame(s) over {} distinct transition cell(s) of {} deck cell(s); \
         the step ONTO each was admitted and held deck height",
        bridgehead_frames.len(),
        bridgehead_cells.len(),
        span.deck.len(),
    );

    // 4. The whole mid-span portion, not just the entry cell.
    let mid_span: Vec<&TickRow> = deck_frames
        .iter()
        .filter(|row| row.cell != span.deck[0] && row.cell != *span.deck.last().unwrap())
        .copied()
        .collect();
    assert!(
        !mid_span.is_empty(),
        "the tank only ever touched the span's end cells; mid-span behaviour is what the \
         report is about"
    );
    println!("{} mid-span frame(s) all at deck height", mid_span.len());

    // 4b. The crossing finished. Reaching the deck and holding the height there
    //     is not the same as getting across: a mover that enters the span and
    //     then stalls, is snapped back, or gives up mid-deck satisfies every
    //     assertion above. The recording loop breaks on the far approach, on
    //     MAX_TICKS, or on an idle run, and only this distinguishes the first
    //     from the other two.
    let last = rows.last().copied().expect("at least one frame recorded");
    assert_eq!(
        last.cell,
        span.approach_b,
        "the mover never reached the far approach {:?}; it stopped at {:?} after {} frame(s)",
        span.approach_b,
        last.cell,
        rows.len()
    );

    // 5. Discrimination. A green run only rules the fix in if the pre-fix height
    //    path would have produced something different on at least one frame;
    //    otherwise it is a compatibility check, not a regression test.
    let discriminating: Vec<&TickRow> = rows.iter().filter(|row| row.prefix_z != row.z).collect();
    if discriminating.is_empty() {
        println!(
            "\nDISCRIMINATION: NONE. On all {} frames the pre-fix formula \
             `effective_cell_z_for_layer(loco.layer)` yields the same z as the fixed \
             `signed_level() + (on_bridge ? 4 : 0)`. This route does NOT reproduce the \
             reported symptom and does NOT distinguish the two implementations — it only \
             shows the fix did not break an ordinary crossing.",
            rows.len()
        );
    } else {
        println!(
            "\nDISCRIMINATION: {} of {} frames differ from the pre-fix formula; first {:?}",
            discriminating.len(),
            rows.len(),
            discriminating[0]
        );
    }
    let ground_layer_on_deck = rows
        .iter()
        .filter(|row| {
            row.structural
                && row.loco_layer == crate::sim::movement::locomotor::MovementLayer::Ground
        })
        .count();
    println!(
        "frames on a deck cell whose live A* layer was Ground (the C1 trigger): {ground_layer_on_deck}"
    );
}

// ---------------------------------------------------------------------------
// Low (`TubeClass`) spans — matrix rows T1-09 / T1-10 / T1-11.
//
// A low span is NOT a high span with different numbers, and it is also not the
// `TubeClass` thing the matrix calls it. Both halves of that are measured by
// `retail_low_bridge_inventory`, whose output is the sole basis for the
// invariant asserted here.
//
// What a low deck cell actually carries, on all four loose low-span fixtures
// (Lostlake, Shrapnel, Killer, EB3):
//
//   has_bridge_deck = true        bridge_deck_level == ground_level  (no +4)
//   bridge_structural = false     bridge_walkable   = false
//   transition        = false     ground_walkable   = true
//   zone_type = 0 (Ground)        LandType  = 1 (Road)   is_water = false
//   tube_index = None             low_bridge_tube_cell = false
//
// The first line is the whole geometry: `resolve_overlay`
// (`map/resolved_terrain.rs`) routes every bridge overlay id that is not
// 24/25/237/238 to `BridgeDirection::Low` and gives it `deck_level = level`,
// and `high_bridge_stamp_for_overlay` (`map/bridge_facts.rs`) returns `None`
// for those ids, so no `0x100` structural stamp is ever written and no Bridge
// A* plane exists over a low span. Both approach cells sit at the deck's own
// level on every fixture, so there is no height event at either end — which is
// why the ledger collapses low onto/along/off into one row.
//
// The last line is the surprise and it is recorded, not worked around: the
// `TubeClass` marker is absent from the entire measured corpus. It is gated on
// `yr_cell_land_type == YR_CELL_LAND_TUNNEL` (10), but a low-bridge overlay
// rewrites the cell's Land to Road (1), and `build_auto_low_bridge_tubes`
// matches *iso-tile* ids, not overlays. So `tube_movement.rs` and
// `PathCell::low_bridge_tube_cell` are not on the stock low-bridge crossing
// path at all. Whether gamemd tubes these cells is UNCHECKED — settling it
// needs the binary, not this harness.
// ---------------------------------------------------------------------------

/// One intact low-bridge span discovered from live map facts.
#[derive(Debug, Clone)]
struct LowBridgeSpan {
    /// Off-deck cell the drive starts on, one step before the first deck cell.
    approach_a: (u16, u16),
    /// Off-deck cell on the far side, one step past the last deck cell.
    approach_b: (u16, u16),
    /// Every low-deck cell between them, in travel order.
    deck: Vec<(u16, u16)>,
    /// The subset of `deck` whose **pre-overlay** terrain is Water — the cells a
    /// mover could not occupy at all if the bridge overlay were removed.
    ///
    /// This is what makes a low-span run a bridge crossing rather than a walk
    /// across flat ground. A low deck is a plain ground cell in every field the
    /// mover reads, so without this the whole test would pass on any straight
    /// stretch of road and would settle nothing.
    water_gap: Vec<(u16, u16)>,
    approach_a_level: u8,
    approach_b_level: u8,
    step: (i32, i32),
}

/// A low deck cell, taken from the map's own overlay classification.
///
/// `resolve_overlay` (`map/resolved_terrain.rs`) maps overlay ids 24/237 to
/// `EastWest`, 25/238 to `NorthSouth` and **every other bridge overlay id** to
/// `Low`, then gives the low ones `deck_level = level` with no `+4`. That is the
/// authoritative low-vs-high split at load, so the span finder reads it rather
/// than guessing from heights — which for a low span are identical to the
/// surrounding terrain and would discriminate nothing.
fn is_low_deck_cell(terrain: &ResolvedTerrainGrid, cell: (u16, u16)) -> bool {
    terrain.cell(cell.0, cell.1).is_some_and(|c| {
        c.bridge_layer
            .as_ref()
            .is_some_and(|layer| layer.direction == BridgeDirection::Low)
    })
}

/// A low deck cell whose **pre-overlay** terrain is Water: a cell that exists as
/// standing room only because the bridge overlay is there.
fn is_water_backed_low_deck(terrain: &ResolvedTerrainGrid, cell: (u16, u16)) -> bool {
    is_low_deck_cell(terrain, cell)
        && terrain.cell(cell.0, cell.1).is_some_and(|c| {
            c.base_land_type == crate::rules::terrain_rules::LandType::Water.as_index()
        })
}

/// Longest straight run of low-deck cells entered and left through ordinary
/// ground-walkable, non-deck cells.
///
/// Deliberately imposes **no** height relation between deck and approach: the
/// high finder demands `approach == deck_terrain + 4` because that is the
/// geometry its predicate fires on, and assuming any analogue here would be
/// assuming the answer. The observed levels are carried on the span and printed.
/// The span the crossing drivers use: longest, ties broken by the lowest first
/// deck cell so the choice is stable across runs and across maps.
///
/// On `Lostlake.mmx` this resolves to `(39,117)..(51,117)` with approaches
/// `(38,117)` and `(52,117)` — the exact fixture the frozen ledger names for
/// T1-09/10/11, confirmed by `retail_low_bridge_inventory` rather than adopted
/// on trust. Nine other 13-cell runs on that map tie on length.
fn find_low_bridge_span(terrain: &ResolvedTerrainGrid, grid: &PathGrid) -> Option<LowBridgeSpan> {
    find_low_bridge_spans(terrain, grid).into_iter().next()
}

/// Every maximal straight low run on the map, deduplicated so a run and its
/// mirror image count once. Used by the inventory: the frozen ledger names one
/// fixture per map, and the only way to check that claim is to enumerate.
fn find_low_bridge_spans(terrain: &ResolvedTerrainGrid, grid: &PathGrid) -> Vec<LowBridgeSpan> {
    let mut seen: std::collections::BTreeSet<((u16, u16), (u16, u16))> =
        std::collections::BTreeSet::new();
    let mut spans = Vec::new();
    for span in enumerate_low_bridge_spans(terrain, grid) {
        let first = span.deck[0];
        let last = *span.deck.last().expect("non-empty run");
        let key = (first.min(last), first.max(last));
        if seen.insert(key) {
            spans.push(span);
        }
    }
    spans.sort_by_key(|span| (std::cmp::Reverse(span.deck.len()), span.deck[0]));
    spans
}

fn enumerate_low_bridge_spans(
    terrain: &ResolvedTerrainGrid,
    grid: &PathGrid,
) -> Vec<LowBridgeSpan> {
    let mut found = Vec::new();
    for y in 0..terrain.height() {
        for x in 0..terrain.width() {
            if !is_low_deck_cell(terrain, (x, y)) {
                continue;
            }
            for step in STEPS {
                let back = (-step.0, -step.1);
                let Some(approach_a) = offset((x, y), back) else {
                    continue;
                };
                if is_low_deck_cell(terrain, approach_a) {
                    continue;
                }
                let Some(cell_a) = grid.cell(approach_a.0, approach_a.1) else {
                    continue;
                };
                if !cell_a.ground_walkable {
                    continue;
                }
                let mut deck = vec![(x, y)];
                let mut cursor = (x, y);
                loop {
                    let Some(next) = offset(cursor, step) else {
                        break;
                    };
                    if !is_low_deck_cell(terrain, next) {
                        break;
                    }
                    deck.push(next);
                    cursor = next;
                }
                let Some(approach_b) = offset(cursor, step) else {
                    continue;
                };
                if is_low_deck_cell(terrain, approach_b) {
                    continue;
                }
                let Some(cell_b) = grid.cell(approach_b.0, approach_b.1) else {
                    continue;
                };
                if !cell_b.ground_walkable {
                    continue;
                }
                let water_gap = deck
                    .iter()
                    .copied()
                    .filter(|cell| is_water_backed_low_deck(terrain, *cell))
                    .collect();
                let candidate = LowBridgeSpan {
                    approach_a,
                    approach_b,
                    deck,
                    water_gap,
                    approach_a_level: cell_a.ground_level,
                    approach_b_level: cell_b.ground_level,
                    step,
                };
                found.push(candidate);
            }
        }
    }
    found
}

fn print_low_inventory(terrain: &ResolvedTerrainGrid, grid: &PathGrid, span: &LowBridgeSpan) {
    println!(
        "span: approach_a {:?} (level {}) -> {} low deck cell(s) -> approach_b {:?} (level {}), \
         step {:?}; {} of the deck cells sit on pre-overlay Water: {:?}",
        span.approach_a,
        span.approach_a_level,
        span.deck.len(),
        span.approach_b,
        span.approach_b_level,
        span.step,
        span.water_gap.len(),
        span.water_gap,
    );
    let mut cells: Vec<(u16, u16)> = vec![span.approach_a];
    cells.extend(span.deck.iter().copied());
    cells.push(span.approach_b);
    for cell in cells {
        let Some(rt) = terrain.cell(cell.0, cell.1) else {
            println!("  {cell:?}: NOT IN RESOLVED TERRAIN");
            continue;
        };
        let Some(pc) = grid.cell(cell.0, cell.1) else {
            println!("  {cell:?}: NOT IN PATH GRID");
            continue;
        };
        let role = if is_low_deck_cell(terrain, cell) {
            "deck"
        } else {
            "appr"
        };
        println!(
            "  {role} {cell:?}: overlay={:?} lvl={} rt_deck_lvl={} rt_has_deck={} rt_bridge_walkable={} \
             tube={:?} tube_src={:?} yr_land={} land={} base_land={} base_blocked={} water={} zone={} \
             | path: gw={} bw={} struct={} trans={} lowtube={} g_lvl={} deck_lvl={}",
            rt.bridge_layer.as_ref().map(|b| b.overlay_id),
            rt.level,
            rt.bridge_deck_level,
            rt.has_bridge_deck,
            rt.bridge_walkable,
            rt.tube_index.map(|t| t.0),
            rt.tube_index
                .and_then(|t| terrain.tube(t))
                .map(|t| t.source),
            rt.yr_cell_land_type,
            rt.land_type,
            rt.base_land_type,
            rt.base_ground_walk_blocked,
            rt.is_water,
            rt.zone_type,
            pc.ground_walkable,
            pc.bridge_walkable,
            pc.bridge_structural,
            pc.transition,
            pc.low_bridge_tube_cell,
            pc.ground_level,
            pc.bridge_deck_level,
        );
    }
}

/// The low-span per-frame invariant, applied to every recorded frame.
///
/// **Measured, not assumed** — see the block comment above for the cell facts
/// `retail_low_bridge_inventory` printed, and note in particular that both
/// approaches sit at the deck's own level on every fixture. There is no height
/// event anywhere on a low span; the crossing *is* the ground plane.
///
/// So the correct invariant is the degenerate case of
/// `ObjectClass::GetHeight @ 0x005F5F30` with OnBridge clear:
///
/// ```text
/// position.z == GroundHeight(own cell)   and   OnBridge == false
/// ```
///
/// The `OnBridge == false` half is load-bearing, not decoration: were a low deck
/// to set it, `Set_Height_On_Bridge` would add four levels of nothing and float
/// the mover over a flat span. Asserting only "z == ground" would pass a
/// hypothetical implementation that sets `on_bridge` and then re-derives z from
/// it, so both halves are checked, plus the absence of a `BridgeOccupancy`
/// entry, which is the third place the deck term is stored.
fn assert_low_span_invariant(rows: &[TickRow], deck_frames: &[&TickRow]) {
    let violations: Vec<&TickRow> = rows.iter().filter(|row| !row.holds_invariant()).collect();
    assert!(
        violations.is_empty(),
        "position.z left the native model on {} frame(s); first: {:?} (expected z {}, got {})",
        violations.len(),
        violations[0],
        violations[0].expected_z(),
        violations[0].z,
    );
    for row in deck_frames {
        assert!(
            !row.on_bridge,
            "a LOW deck cell {:?} set on_bridge; there is no deck plane over a low span, so \
             Set_Height_On_Bridge would lift the mover four levels above flat ground: {row:?}",
            row.cell
        );
        assert!(
            !row.structural,
            "a low deck cell {:?} reported bridge_structural; the low overlays get no \
             SetBridgeDirection stamp: {row:?}",
            row.cell
        );
        assert_eq!(
            i16::from(row.z as i8),
            i16::from(row.terrain_level as i8),
            "the mover is not at ground height on low deck cell {:?}: {row:?}",
            row.cell
        );
        assert_eq!(
            row.occupancy_deck, None,
            "a low deck cell {:?} produced a BridgeOccupancy entry: {row:?}",
            row.cell
        );
        assert_eq!(
            row.stored_deck_level, row.terrain_level,
            "the stored per-cell deck level on low cell {:?} is not the ground level: {row:?}",
            row.cell
        );
    }
}

/// Load a retail map, spawn `unit_type` on one approach of the longest intact
/// low span, order it to the far approach with an ordinary undisabled
/// `Command::Move`, and record `position.z` after every committed frame.
///
/// Same contract as `drive_across_high_bridge` — a refused order panics after a
/// diagnosis rather than being re-issued through a widened search — with the low
/// invariant of `assert_low_span_invariant` in place of the deck-height one.
fn drive_across_low_bridge(map_file: &str, unit_type: &str) {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let mut scenario = match headless_scenario::load(&retail, map_file, SEED) {
        Ok(scenario) => scenario,
        Err(error) => panic!("load {map_file}: {error}"),
    };
    println!(
        "loaded {map_file} ({}x{}, theater {})",
        scenario.sim().session.map_width,
        scenario.sim().session.map_height,
        scenario.map.header.theater,
    );

    let span = {
        let sim = scenario.sim();
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("headless load keeps resolved terrain");
        let grid = sim.path_grid().expect("headless load publishes navigation");
        let Some(span) = find_low_bridge_span(terrain, grid) else {
            panic!("{map_file} exposes no low-bridge span");
        };
        print_low_inventory(terrain, grid, &span);
        span
    };
    assert!(
        span.deck.len() >= 3,
        "a two-cell span proves nothing about mid-span behaviour; found {} deck cell(s)",
        span.deck.len()
    );

    let owner_name = prepare_commanding_house(&mut scenario);
    println!("commanding house: {owner_name}");

    let start_candidates = [
        span.approach_a,
        offset(span.approach_a, (-span.step.0, -span.step.1)).unwrap_or(span.approach_a),
    ];
    let mut entity_id = None;
    let mut start_cell = span.approach_a;
    for candidate in start_candidates {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        if let Some(id) = simulation.spawn_object(
            unit_type,
            &owner_name,
            candidate.0,
            candidate.1,
            0,
            &resources.rules,
            &resources.height_map,
        ) {
            entity_id = Some(id);
            start_cell = candidate;
            break;
        }
    }
    let entity_id = entity_id.unwrap_or_else(|| {
        panic!("could not place a {unit_type} on either approach cell {start_candidates:?}")
    });
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    let movement_zone = {
        let entity = scenario
            .sim()
            .entities()
            .get(entity_id)
            .expect("spawned mover present");
        let zone = entity
            .locomotor
            .as_ref()
            .map(|loco| loco.movement_zone)
            .expect("spawned mover has a locomotor");
        println!(
            "spawned {unit_type} id={entity_id} at {start_cell:?} z={} on_bridge={} zone={zone:?}",
            entity.position.z, entity.on_bridge
        );
        zone
    };
    // Whether the river under the span is an obstacle to *this* mover. A
    // Robot Tank is `AmphibiousDestroyer` and crosses open water unaided, so for
    // it the water gap cannot be used as proof that the bridge was needed.
    let water_is_an_obstacle = !matches!(
        movement_zone,
        MovementZone::Amphibious
            | MovementZone::AmphibiousCrusher
            | MovementZone::AmphibiousDestroyer
            | MovementZone::Water
            | MovementZone::WaterBeach
    );

    let owner_id = scenario
        .sim()
        .interner
        .get(&owner_name)
        .expect("owner interned");
    let execute_tick = scenario.sim().session.tick + 1;
    let order = CommandEnvelope::new(
        owner_id,
        execute_tick,
        Command::Move {
            entity_id,
            target_rx: span.approach_b.0,
            target_ry: span.approach_b.1,
            queue: false,
            group_id: None,
        },
    );
    scenario
        .runtime
        .advance_frame(&[order], SIM_TICK_MS, TickLane::Ordinary)
        .expect("fixture frame must complete");

    match scenario
        .sim()
        .entities()
        .get(entity_id)
        .and_then(|entity| entity.movement_target.as_ref())
        .map(|target| target.path.clone())
    {
        Some(path) => println!(
            "move order accepted: {} node(s) {:?} -> {:?}",
            path.len(),
            path.first(),
            path.last()
        ),
        None => {
            diagnose_rejected_order(
                &mut scenario,
                &owner_name,
                entity_id,
                start_cell,
                span.approach_a,
                span.approach_b,
                &span.deck,
            );
            panic!(
                "the ordinary Command::Move to {:?} was REFUSED — a player cannot cross this low \
                 span at all. See the diagnosis above.",
                span.approach_b
            );
        }
    }

    let mut rows: Vec<TickRow> = Vec::new();
    let mut idle_frames = 0u32;
    for _ in 0..MAX_TICKS {
        scenario.tick();
        let sim = scenario.sim();
        let Some(entity) = sim.entities().get(entity_id) else {
            panic!("the mover vanished mid-crossing");
        };
        let cell = (entity.position.rx, entity.position.ry);
        let facts = sim
            .path_grid()
            .and_then(|grid| grid.cell(cell.0, cell.1))
            .copied();
        let Some(facts) = facts else {
            panic!("the mover left the path grid at {cell:?}");
        };
        let loco_layer = entity.movement_layer_or_ground();
        rows.push(TickRow {
            tick: sim.session.tick,
            cell,
            z: entity.position.z,
            on_bridge: entity.on_bridge,
            occupancy_deck: entity.bridge_occupancy.map(|occ| occ.deck_level),
            terrain_level: facts.ground_level,
            structural: facts.bridge_structural,
            bridge_walkable: facts.bridge_walkable,
            transition: facts.transition,
            low_tube: facts.low_bridge_tube_cell,
            stored_deck_level: facts.bridge_deck_level,
            loco_layer,
            prefix_z: facts.effective_cell_z_for_layer(loco_layer),
        });
        if entity.movement_target.is_none() {
            idle_frames += 1;
            if idle_frames >= 30 {
                break;
            }
        } else {
            idle_frames = 0;
        }
        if cell == span.approach_b {
            break;
        }
    }

    print_tick_table(&rows);
    let last = rows.last().copied().expect("at least one frame observed");
    println!(
        "\n{} frame(s) recorded, last cell {:?} (target {:?})",
        rows.len(),
        last.cell,
        span.approach_b
    );

    let terrain = scenario
        .sim()
        .resolved_terrain
        .as_ref()
        .expect("headless load keeps resolved terrain");

    // 1. The drive must actually have used the span.
    //
    // Deck membership is "the mover's cell is a low deck cell", not "the cell is
    // in the discovered single-file run". MEASURED reason: a low span is often
    // several lanes wide — Killer's is three, columns 92/93/94 — and A* is free
    // to change lane mid-crossing between equal-cost parallel deck cells. The
    // Hover mover does exactly that on Killer, leaving the discovered lane at
    // y=136 and rejoining it at y=144. That is ordinary tie-breaking, not a
    // defect, but a single-lane membership test calls it a failed crossing.
    let deck_frames: Vec<&TickRow> = rows
        .iter()
        .filter(|row| is_low_deck_cell(terrain, row.cell))
        .collect();
    assert!(
        !deck_frames.is_empty(),
        "the {unit_type} never stood on a low deck cell — it did not cross the span, so this run \
         says nothing about low-span movement. Cells visited: {:?}",
        rows.iter().map(|row| row.cell).collect::<Vec<_>>()
    );
    let visited_deck: std::collections::BTreeSet<(u16, u16)> =
        deck_frames.iter().map(|row| row.cell).collect();
    println!(
        "stood on {} low deck cell(s) ({} of the {} in the discovered lane): {:?}",
        visited_deck.len(),
        visited_deck
            .iter()
            .filter(|cell| span.deck.contains(cell))
            .count(),
        span.deck.len(),
        visited_deck
    );

    // 1b. It was a *bridge* crossing. A low deck reads as a plain ground cell in
    //     every field the mover consults, so reaching the far approach proves
    //     nothing on its own — the same run would pass on a straight stretch of
    //     road. What makes it a crossing is that the middle of the span is water
    //     under the overlay, and the mover was over all of it.
    //
    //     Coverage is measured along the travel axis rather than cell by cell,
    //     which is what makes it lane-agnostic: for every position along the
    //     river the span bridges, the mover must have stood on *some*
    //     water-backed deck cell at that position.
    assert!(
        !span.water_gap.is_empty(),
        "this span has no pre-overlay water under it, so crossing it says nothing about \
         bridges; deck {:?}",
        span.deck
    );
    let axis = |cell: (u16, u16)| if span.step.0 != 0 { cell.0 } else { cell.1 };
    let gap_axis: std::collections::BTreeSet<u16> =
        span.water_gap.iter().copied().map(axis).collect();
    let covered_axis: std::collections::BTreeSet<u16> = visited_deck
        .iter()
        .copied()
        .filter(|cell| is_water_backed_low_deck(terrain, *cell))
        .map(axis)
        .collect();
    if water_is_an_obstacle {
        let uncovered: Vec<u16> = gap_axis.difference(&covered_axis).copied().collect();
        assert!(
            uncovered.is_empty(),
            "the mover was never over the river at {uncovered:?} along the travel axis; the span \
             bridges water at {gap_axis:?} and the mover only covered {covered_axis:?}"
        );
        println!(
            "spanned the whole {}-wide water gap at axis positions {gap_axis:?}",
            gap_axis.len()
        );
    } else {
        // MEASURED, and it is a real weakening of this row rather than a
        // convenience: on `Killer.mmx` the `ROBO` leaves the deck at y=136 and
        // hovers the river directly to y=143 before rejoining, because
        // `AmphibiousDestroyer` treats the water as passable and the two routes
        // cost near enough the same. On `Lostlake.mmx` the same unit stays on
        // all 13 deck cells. Neither is a defect; both mean the water gap
        // cannot certify that an amphibious mover *needed* the bridge.
        //
        // What is still asserted is that it used the bridge over open water
        // somewhere, which rules out a run that bypassed the span entirely.
        assert!(
            !covered_axis.is_empty(),
            "the {unit_type} ({movement_zone:?}) never stood on a water-backed deck cell, so it \
             did not use the span at all; the span bridges water at {gap_axis:?}"
        );
        println!(
            "NOTE: {unit_type} is {movement_zone:?} and crosses open water unaided, so full \
             water-gap coverage is NOT required of it. Covered {} of the {} axis positions: \
             {covered_axis:?}",
            covered_axis.len(),
            gap_axis.len(),
        );
    }

    // 2/3. The measured low-span invariant.
    assert_low_span_invariant(&rows, &deck_frames);

    // 4. Mid-span, not just the two end cells. The matrix collapses low
    //    onto/along/off into one row precisely because there is no height event
    //    at either end, so a run that only clipped the ends would settle nothing.
    let mid_span: Vec<&TickRow> = deck_frames
        .iter()
        .filter(|row| row.cell != span.deck[0] && row.cell != *span.deck.last().unwrap())
        .copied()
        .collect();
    assert!(
        !mid_span.is_empty(),
        "the mover only ever touched the span's end cells; mid-span behaviour is the row"
    );
    println!("{} mid-span frame(s) all at ground height", mid_span.len());

    // 5. The crossing finished. Entering the span and holding height on it is not
    //    the same as getting across.
    assert_eq!(
        last.cell,
        span.approach_b,
        "the mover never reached the far approach {:?}; it stopped at {:?} after {} frame(s)",
        span.approach_b,
        last.cell,
        rows.len()
    );

    let tube_frames = rows.iter().filter(|row| row.low_tube).count();
    println!(
        "frames on a cell carrying the TubeClass low-bridge marker: {tube_frames} of {}",
        rows.len()
    );
}

/// Matrix row T1-09 — Drive along an intact low span, on the exact fixture the
/// ledger names: `Lostlake.mmx` `(39,117)..(51,117)`, ordered `(38,117)` →
/// `(52,117)`. No low span had been crossed by any locomotor anywhere in this
/// project; low spans are on ~32 % of stock maps.
///
/// Stands for the collapsed onto/along/off set — a low deck sits at the
/// surrounding terrain level (measured: both approaches share the deck's level
/// on all four fixtures), so there is no height event at either end and one
/// crossing exercises all three relations.
///
/// It does **not** stand for the wood/concrete pair the way the ledger assumed.
/// That collapse was justified by both materials sharing the `TubeClass`
/// movement path; the inventory shows neither of them reaches it. What they do
/// share is the ordinary ground plane, which is a stronger reason for the same
/// collapse — but it is a different reason, and only the wood fixture is loose.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_crosses_lostlake_low_bridge_at_ground_height() {
    drive_across_low_bridge("Lostlake.mmx", "MTNK");
}

/// Matrix row T1-10 — Walk along the same span.
///
/// Still separate from T1-09, but **not for the reason the ledger gives.** The
/// row cites the `EntityCategory::{Unit, Infantry}` gate in
/// `begin_path_tube_step` (`tube_movement.rs`) as infantry's own arm; the
/// inventory shows no stock low-bridge cell carries a tube index, so that gate
/// is never reached on any of these maps and cannot be what separates the rows.
///
/// What does separate them is the same thing that separated T1-06 from T1-03:
/// infantry take a sub-cell reservation arm no vehicle touches, and drive the
/// Walk locomotion twin rather than the drive track. Neither had ever been run
/// over a low deck.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn infantry_crosses_lostlake_low_bridge_at_ground_height() {
    drive_across_low_bridge("Lostlake.mmx", "E1");
}

/// Matrix row T1-11 — Hover along the same span.
///
/// The ledger notes `tube_movement.rs` gates on category and layer, never on
/// locomotor kind — which the inventory makes moot, since a stock low span never
/// enters that file. The row stands on the part that was always the real point:
/// the planner, the crossing loop and the height commit all sit upstream, and
/// T1-01 showed Hover taking a different planner branch from Drive on a high
/// span and stalling indefinitely for it. "Hover is the same as Drive here" is
/// exactly the kind of assumption this row exists to measure.
///
/// **This row is weaker than T1-09 and T1-10, on purpose.** `ROBO` is
/// `AmphibiousDestroyer`, so the river the span bridges is not an obstacle to
/// it, and the water-gap coverage check that certifies the Drive and Walk rows
/// cannot certify this one. Measured: on `Lostlake.mmx` it stays on all 13 deck
/// cells and covers the whole 7-cell gap; on `Killer.mmx` it leaves the deck at
/// y=136 and hovers open water to y=143 before rejoining, covering 6 of 14. Both
/// are legitimate. What this row therefore settles is that an ordinary Move
/// across a low span is accepted, that the mover holds ground height with no
/// `on_bridge` and no `BridgeOccupancy` on every deck cell it does use, and that
/// it arrives — not that it needed the bridge.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn hover_tank_crosses_lostlake_low_bridge_at_ground_height() {
    drive_across_low_bridge("Lostlake.mmx", "ROBO");
}

// The second low-span geometry, for the same reason the high rows carry two
// maps. `Lostlake.mmx`'s fixture runs east-west across a 13-cell span with a
// 7-cell water gap and Road approaches; `Killer.mmx` runs north-south across 22
// cells with a 14-cell water gap and Rough approaches. A row settled on one axis
// and one approach LandType is settled on one map, not on low spans.

#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_crosses_killer_low_bridge_at_ground_height() {
    drive_across_low_bridge("Killer.mmx", "MTNK");
}

#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn infantry_crosses_killer_low_bridge_at_ground_height() {
    drive_across_low_bridge("Killer.mmx", "E1");
}

#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn hover_tank_crosses_killer_low_bridge_at_ground_height() {
    drive_across_low_bridge("Killer.mmx", "ROBO");
}

/// Inventory only: where the stock maps put low spans and what facts those cells
/// carry. Diagnostic — it asserts nothing about movement, and exists because the
/// low-span invariant had to be measured before it could be written down.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_low_bridge_inventory() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    for map_file in [
        "Lostlake.mmx",
        "Shrapnel.mmx",
        "Killer.mmx",
        "EB3.mmx",
        "Dustbowl.mmx",
        "Bermuda.mmx",
        "BayOPigs.mmx",
        "Hills.mmx",
    ] {
        println!("\n===== {map_file} =====");
        match headless_scenario::load(&retail, map_file, SEED) {
            Ok(scenario) => {
                let sim = scenario.sim();
                let terrain = sim
                    .resolved_terrain
                    .as_ref()
                    .expect("headless load keeps resolved terrain");
                let grid = sim.path_grid().expect("navigation published");
                let all: Vec<(u16, u16)> = (0..terrain.height())
                    .flat_map(|y| (0..terrain.width()).map(move |x| (x, y)))
                    .collect();
                let low = all
                    .iter()
                    .filter(|c| is_low_deck_cell(terrain, **c))
                    .count();
                let structural = all
                    .iter()
                    .filter(|(x, y)| grid.cell(*x, *y).is_some_and(|c| c.bridge_structural))
                    .count();
                let tube_marked = all
                    .iter()
                    .filter(|(x, y)| grid.cell(*x, *y).is_some_and(|c| c.low_bridge_tube_cell))
                    .count();
                let tube_indexed = all
                    .iter()
                    .filter(|(x, y)| terrain.cell(*x, *y).is_some_and(|c| c.tube_index.is_some()))
                    .count();
                println!(
                    "{}x{}: {low} low-deck cell(s), {structural} structural high cell(s), \
                     {tube_marked} low_bridge_tube_cell, {tube_indexed} with a tube index",
                    terrain.width(),
                    terrain.height(),
                );
                let spans = find_low_bridge_spans(terrain, grid);
                println!("{} crossable low run(s), longest first:", spans.len());
                for span in &spans {
                    println!(
                        "  {:?} .. {:?} ({} cells, step {:?}) approaches {:?} lvl {} / {:?} lvl {}",
                        span.deck[0],
                        span.deck.last().expect("non-empty"),
                        span.deck.len(),
                        span.step,
                        span.approach_a,
                        span.approach_a_level,
                        span.approach_b,
                        span.approach_b_level,
                    );
                }
                match spans.first() {
                    Some(span) => print_low_inventory(terrain, grid, span),
                    None => println!("no usable low span"),
                }
            }
            Err(error) => println!("{map_file}: load failed: {error}"),
        }
    }
}

#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_crosses_bay_of_pigs_high_bridge_at_deck_height() {
    drive_across_high_bridge("BayOPigs.mmx", "MTNK", true);
}

/// Second retail map, different theater and a longer span, so the result is not
/// one map's geometry.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_crosses_hills_high_bridge_at_deck_height() {
    drive_across_high_bridge("Hills.mmx", "MTNK", true);
}

/// Matrix row T1-01. The Robot Tank is a **Hover** locomotor, and until
/// `supports_layered_bridge_pathing` (`movement_path.rs`) admitted Hover it was
/// excluded from the layered path builder, received the flat fallback path whose
/// `path_layers` are `Ground` on every node including the deck cells, and was
/// then refused at the crossing loop's terrain test — which read the riverbed
/// under the span. The order was never dropped, so it drove into the abutment
/// and was snapped back to cell centre indefinitely.
///
/// This was a characterization test asserting that stall. It is now a positive
/// crossing, holding the Hover mover to exactly what the Drive crossings assert:
/// it reaches the deck at all, stands mid-span rather than only on the end
/// cells, and on every deck frame carries `on_bridge`, `z == terrain + 4`, a
/// height that is not the riverbed, and a `BridgeOccupancy` agreeing with its
/// own `z`. `drive_across_high_bridge` also now asserts arrival at the far
/// approach, so a mover that reaches the deck and then dies mid-span fails.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn hover_tank_crosses_bay_of_pigs_high_bridge_at_deck_height() {
    drive_across_high_bridge("BayOPigs.mmx", "ROBO", true);
}

/// The Hover twin on a second map's geometry. `BayOPigs.mmx` runs its span
/// north-south down a column and `Hills.mmx` east-west along a row, so a single
/// map would leave a Hover crossing certified on one span's axis and levels.
/// The Drive side is covered on both; this closes the same gap for Hover.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn hover_tank_crosses_hills_high_bridge_at_deck_height() {
    drive_across_high_bridge("Hills.mmx", "ROBO", true);
}

/// Matrix rows T1-06, T1-07 and T1-08 — Walk onto, along and off an intact high
/// span. Infantry had never been driven across a bridge anywhere in this
/// project: the harness had only ever moved `MTNK` and `ROBO`, so the native
/// twin `WalkLocomotionClass::ProcessMovement` `0x0075C154`-`0x0075C199` was
/// unexercised on the Rust side.
///
/// Walk is not a collapse of the Drive rows. Infantry take a sub-cell
/// reservation arm no vehicle touches, and it is the one Walk-specific bridge
/// code path — so a Drive crossing says nothing about whether an `E1` can hold
/// a sub-cell slot on a deck.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn infantry_crosses_bay_of_pigs_high_bridge_at_deck_height() {
    drive_across_high_bridge("BayOPigs.mmx", "E1", true);
}

#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn infantry_crosses_hills_high_bridge_at_deck_height() {
    drive_across_high_bridge("Hills.mmx", "E1", true);
}

// ---------------------------------------------------------------------------
// Attack-move across an intact high span — matrix rows T2-01 and T2-04.
//
// Evidence gap 5 said "order sources other than a single ordinary Move have
// zero observations". These are the first attack-move crossings in the project.
//
// What an attack-move adds over a Move, read from `world_commands.rs`: the
// `AttackMove` arm and the `Move` arm hand `movement::issue_move_command_with_layered`
// the *same* fourteen arguments, so the path build is not a separate code path
// and the A* goal is resolved identically. What differs is the order machinery
// wrapped round it — `queue_megamission_with_teardown(MissionType::AttackMove)`,
// the `attack_target` / `passively_acquired_target` clear, and the
// `OrderIntent::AttackMove` stamp, which arms a per-tick resume in
// `tick_order_intents_post_combat_with_overlay_registry` (`world_orders.rs`)
// that re-issues the move from wherever the unit is standing every time it
// finds itself with no attack target and no movement target.
//
// That resume is the reason these rows are worth running rather than collapsing
// onto T1-03/T1-01 by argument-list inspection alone: it re-plans mid-crossing
// from a *deck cell*, with a reduced argument set (no terrain cost grid, no
// entity blocks, no entity block map, `mover_is_crusher = false`), which is a
// combination no Move ever produces.
// ---------------------------------------------------------------------------

/// Matrix rows T2-01 (Drive) and T2-04 (Hover) — attack-move onto an intact high
/// span, as positive crossings.
///
/// **Settled 2026-09-15.** The four `..._is_currently_dropped` characterizations
/// that pinned the attack-move stall went red on `main`: the probe they wrapped
/// (`AttackMoveProbe`, retired with them) reported the mover crossing every deck
/// cell under the attack-move on both geometries and for all three locomotors
/// (Hills/MTNK 24 cells and 245 surviving ticks, Hills/E1 24 cells, BayOPigs/MTNK
/// and /ROBO 19 cells), with the plain-Move control crossing too. The rows are
/// therefore held to the same crossing contract as the Move rows, through the
/// shared driver with the attack-move order source. Rust movement regression,
/// not a native golden: no attack-move oracle exists.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_attack_moved_across_bay_of_pigs_high_bridge_crosses() {
    drive_across_high_bridge_with_order("BayOPigs.mmx", "MTNK", true, OrderSource::AttackMove);
}

/// The second geometry — BayOPigs runs its span north-south down a column,
/// Hills east-west along a row — so the result is not one map's arrangement.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_attack_moved_across_hills_high_bridge_crosses() {
    drive_across_high_bridge_with_order("Hills.mmx", "MTNK", true, OrderSource::AttackMove);
}

/// Matrix row T2-04 — the Hover arm. Hover shares the planner entry with Drive
/// since `53695936`; the row is kept because attack-move goal selection was the
/// half nothing had measured.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn hover_tank_attack_moved_across_bay_of_pigs_high_bridge_crosses() {
    drive_across_high_bridge_with_order("BayOPigs.mmx", "ROBO", true, OrderSource::AttackMove);
}

/// The Walk arm, so T2-01's "Stands for Walk" clause is measured rather than
/// assumed.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn infantry_attack_moved_across_hills_high_bridge_crosses() {
    drive_across_high_bridge_with_order("Hills.mmx", "E1", true, OrderSource::AttackMove);
}

// ---------------------------------------------------------------------------
// Repath after a block, ON the deck — matrix row T2-03.
//
// `try_repath_after_block` (`movement_path.rs`) is reached from
// `movement_blocked.rs` when a mover's next step is occupied. It is untested on
// a deck anywhere in the tree, and the row's question is whether the repath it
// produces stays on the Bridge layer or silently drops the remaining path to
// Ground — which on a high span means the route is re-planned against the
// riverbed under the mover's feet.
//
// The blocker is *driven* onto the deck with its own ordinary Move rather than
// spawned there. `spawn_object` has no bridge-deck term (matrix N-01: a unit
// spawned on `BayOPigs (111,143)` gets `z=1, on_bridge=false` on a cell whose
// deck is at 5, i.e. under the span), so a spawned blocker would be an
// under-span obstacle and would settle nothing about the deck plane.
// ---------------------------------------------------------------------------

/// Matrix row T2-03 — Drive, high intact, along, **bump / repath-after-block**.
///
/// Two tanks, one span. The first is driven to a mid-span deck cell and left
/// parked there; the second is then ordered across the same span, so its route
/// runs into a stationary mover standing on the deck in front of it.
///
/// What is asserted, in order of what each rules out:
///
/// 1. The blocker genuinely reached the deck and stopped there at deck height —
///    otherwise there is no deck-plane obstacle and the rest proves nothing.
///    This is also the only observation in the project of a drive track that
///    *terminates* on a deck cell (residual R-T105's trigger).
/// 2. Every frame of the crosser obeys the native height model.
/// 3. On every frame the crosser spends on a structural cell, no node of its
///    live `path_layers` that lands on a structural cell is `Ground`. This is
///    the row's named check: a repath that dropped to the ground plane would
///    show up here as a Ground-layered node on a stamped cell.
/// 4. Repathing actually happened — the crosser's path was rebuilt at least
///    once while it was on the deck. Without this the test could pass by the
///    two units never meeting.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_repathing_around_a_deck_blocker_stays_on_the_bridge_layer() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let map_file = "BayOPigs.mmx";
    let mut scenario = match headless_scenario::load(&retail, map_file, SEED) {
        Ok(scenario) => scenario,
        Err(error) => panic!("load {map_file}: {error}"),
    };
    let span = {
        let grid = scenario.sim().path_grid().expect("navigation published");
        find_high_bridge_span(grid).unwrap_or_else(|| panic!("{map_file} exposes no span"))
    };
    let owner_name = prepare_commanding_house(&mut scenario);
    let park_cell = span.deck[span.deck.len() / 2];
    println!(
        "span {:?}..{:?}, blocker parks mid-span at {park_cell:?}",
        span.deck.first(),
        span.deck.last(),
    );

    // --- 1. Drive the blocker onto the deck and park it there. ---
    let blocker = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation
            .spawn_object(
                "MTNK",
                &owner_name,
                span.approach_a.0,
                span.approach_a.1,
                0,
                &resources.rules,
                &resources.height_map,
            )
            .expect("blocker placed on the near approach")
    };
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    assert!(
        issue_ordinary_move(&mut scenario, &owner_name, blocker, park_cell),
        "the blocker's ordinary Move onto the deck at {park_cell:?} was refused"
    );
    let blocker_rows = record_until(&mut scenario, blocker, park_cell);
    let blocker_last = *blocker_rows.last().expect("blocker recorded frames");
    println!(
        "blocker: {} frame(s), last {:?} z={} on_bridge={} structural={}",
        blocker_rows.len(),
        blocker_last.cell,
        blocker_last.z,
        blocker_last.on_bridge,
        blocker_last.structural,
    );
    assert_eq!(
        blocker_last.cell, park_cell,
        "the blocker never reached the mid-span cell, so no deck-plane obstacle exists"
    );
    assert!(
        blocker_last.structural && blocker_last.on_bridge,
        "the blocker stopped on {park_cell:?} without being on the deck: {blocker_last:?}"
    );
    assert_eq!(
        i16::from(blocker_last.z as i8),
        i16::from(blocker_last.terrain_level as i8) + BRIDGE_DECK_LEVEL_DELTA,
        "a track TERMINATING on a deck cell left the mover off deck height: {blocker_last:?}"
    );
    // Settle it: let the parked mover idle a while and confirm it stays put and
    // stays at deck height, so it is an obstacle for the whole of the next run.
    for _ in 0..30 {
        scenario.tick();
    }

    // --- 2. Order the crosser into it. ---
    let crosser = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation
            .spawn_object(
                "MTNK",
                &owner_name,
                span.approach_a.0,
                span.approach_a.1,
                0,
                &resources.rules,
                &resources.height_map,
            )
            .or_else(|| {
                let back = offset(span.approach_a, (-span.step.0, -span.step.1))?;
                simulation.spawn_object(
                    "MTNK",
                    &owner_name,
                    back.0,
                    back.1,
                    0,
                    &resources.rules,
                    &resources.height_map,
                )
            })
            .expect("crosser placed behind the span")
    };
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    assert!(
        issue_ordinary_move(&mut scenario, &owner_name, crosser, span.approach_b),
        "the crosser's ordinary Move across the span was refused"
    );

    // --- 3. Record, watching the live path layers every frame. ---
    let mut rows: Vec<TickRow> = Vec::new();
    let mut path_rebuilds = 0usize;
    let mut rebuilds_while_on_deck = 0usize;
    let mut ground_nodes_on_stamped_cells: Vec<((u16, u16), usize)> = Vec::new();
    let mut previous_path: Option<Vec<(u16, u16)>> = None;
    let mut idle_frames = 0u32;
    for _ in 0..MAX_TICKS {
        scenario.tick();
        let sim = scenario.sim();
        let Some(entity) = sim.entities().get(crosser) else {
            panic!("the crosser vanished");
        };
        let cell = (entity.position.rx, entity.position.ry);
        let Some(facts) = sim
            .path_grid()
            .and_then(|grid| grid.cell(cell.0, cell.1))
            .copied()
        else {
            panic!("the crosser left the path grid at {cell:?}");
        };
        let loco_layer = entity.movement_layer_or_ground();
        let row = TickRow {
            tick: sim.session.tick,
            cell,
            z: entity.position.z,
            on_bridge: entity.on_bridge,
            occupancy_deck: entity.bridge_occupancy.map(|occ| occ.deck_level),
            terrain_level: facts.ground_level,
            structural: facts.bridge_structural,
            bridge_walkable: facts.bridge_walkable,
            transition: facts.transition,
            low_tube: facts.low_bridge_tube_cell,
            stored_deck_level: facts.bridge_deck_level,
            loco_layer,
            prefix_z: facts.effective_cell_z_for_layer(loco_layer),
        };
        rows.push(row);

        // The row's named check: every node of the live path that sits on a
        // stamped cell must be Bridge-layered. A repath that dropped the
        // remaining route to the ground plane shows up here.
        if let Some(target) = entity.movement_target.as_ref() {
            if previous_path.as_ref() != Some(&target.path) {
                path_rebuilds += 1;
                if row.structural {
                    rebuilds_while_on_deck += 1;
                }
                previous_path = Some(target.path.clone());
            }
            let grid = sim.path_grid().expect("navigation published");
            for (index, node) in target.path.iter().enumerate() {
                let stamped = grid
                    .cell(node.0, node.1)
                    .is_some_and(|c| c.bridge_structural);
                let layer = target.path_layers.get(index).copied();
                if stamped
                    && layer == Some(crate::sim::movement::locomotor::MovementLayer::Ground)
                    && index >= target.next_index
                {
                    ground_nodes_on_stamped_cells.push((*node, index));
                }
            }
            idle_frames = 0;
        } else {
            idle_frames += 1;
            if idle_frames >= 30 {
                break;
            }
        }
        if cell == span.approach_b {
            break;
        }
    }
    print_tick_table(&rows);

    let deck_frames: Vec<&TickRow> = rows.iter().filter(|row| row.structural).collect();
    let last = *rows.last().expect("crosser recorded frames");
    println!(
        "\ncrosser: {} frame(s), {} on the deck, last {:?}; {path_rebuilds} path rebuild(s), \
         {rebuilds_while_on_deck} of them while standing on a stamped cell",
        rows.len(),
        deck_frames.len(),
        last.cell,
    );

    let violations: Vec<&TickRow> = rows.iter().filter(|row| !row.holds_invariant()).collect();
    assert!(
        violations.is_empty(),
        "position.z left the native model on {} frame(s); first {:?}",
        violations.len(),
        violations[0],
    );
    assert!(
        !deck_frames.is_empty(),
        "the crosser never reached the deck, so the blocker was never in front of it on the \
         bridge layer; cells visited {:?}",
        rows.iter().map(|row| row.cell).collect::<Vec<_>>()
    );
    assert!(
        ground_nodes_on_stamped_cells.is_empty(),
        "a repath put {} un-traversed path node(s) on the GROUND layer of a stamped bridge \
         cell — the remaining route was re-planned against the riverbed under the span: {:?}",
        ground_nodes_on_stamped_cells.len(),
        ground_nodes_on_stamped_cells,
    );
    assert!(
        rebuilds_while_on_deck > 0,
        "no path rebuild happened while the crosser stood on a stamped cell, so \
         `try_repath_after_block` was never exercised on a deck and this run settles nothing \
         ({path_rebuilds} rebuild(s) total)"
    );
    for row in &deck_frames {
        assert!(
            row.on_bridge,
            "deck frame without on_bridge during the blocked crossing: {row:?}"
        );
    }
    println!(
        "T2-03: {rebuilds_while_on_deck} on-deck repath(s), 0 Ground-layered nodes on stamped \
         cells, last cell {:?} (target {:?})",
        last.cell, span.approach_b
    );
}

// ---------------------------------------------------------------------------
// Non-pristine spans — matrix rows T2-08 / T2-09 / T2-10.
//
// Inventory only: what the loaded map actually exposes at the cells the frozen
// ledger names, so the crossing tests below assert measured geometry instead of
// the ledger's second-hand cell list.
// ---------------------------------------------------------------------------

/// Print, for one map, every structural run that is **broken** — a straight line
/// of stamped cells interrupted by one or more unstamped cells at the same
/// terrain level, i.e. a partially collapsed high span — plus the raw bridge
/// facts of every low-deck cell whose overlay id is not a pristine body id.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_damaged_bridge_inventory() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    for map_file in ["Deadman.mmx", "YuriPlot.mmx", "Shrapnel.mmx"] {
        let Ok(scenario) = headless_scenario::load(&retail, map_file, SEED) else {
            println!("{map_file}: load failed");
            continue;
        };
        let sim = scenario.sim();
        let grid = sim.path_grid().expect("navigation published");
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("headless load keeps resolved terrain");
        println!("\n=== {map_file} ({}x{}) ===", grid.width(), grid.height());

        // High: structural cells, grouped into straight runs, reporting gaps.
        let mut structural: Vec<(u16, u16)> = Vec::new();
        for y in 0..grid.height() {
            for x in 0..grid.width() {
                if grid.cell(x, y).is_some_and(|c| c.bridge_structural) {
                    structural.push((x, y));
                }
            }
        }
        println!("  {} structural cell(s)", structural.len());
        // Rows that contain structural cells with a hole in them.
        let mut rows: std::collections::BTreeMap<u16, Vec<u16>> = std::collections::BTreeMap::new();
        for (x, y) in &structural {
            rows.entry(*y).or_default().push(*x);
        }
        for (y, xs) in &rows {
            let (min, max) = (xs[0], *xs.last().expect("non-empty"));
            let holes: Vec<u16> = (min..=max).filter(|x| !xs.contains(x)).collect();
            if holes.is_empty() {
                continue;
            }
            println!("  row y={y}: structural x={min}..={max} with HOLE(S) at {holes:?}");
            for x in min..=max {
                let Some(pc) = grid.cell(x, *y) else { continue };
                let rt = terrain.cell(x, *y);
                println!(
                    "    ({x},{y}) struct={} bw={} trans={} gw={} g_lvl={} deck_lvl={} \
                     state={:?} overlay={:?} raw_flags={:?}",
                    pc.bridge_structural,
                    pc.bridge_walkable,
                    pc.transition,
                    pc.ground_walkable,
                    pc.ground_level,
                    pc.bridge_deck_level,
                    rt.map(|c| c.bridge_facts.state_byte),
                    rt.and_then(|c| c.bridge_layer.as_ref().map(|b| b.overlay_id)),
                    rt.map(|c| c.bridge_facts.raw_flags),
                );
            }
        }

        // Low: any low-deck cell whose overlay id is outside the pristine body
        // range the intact fixtures use.
        let mut low_by_overlay: std::collections::BTreeMap<u8, Vec<(u16, u16)>> =
            std::collections::BTreeMap::new();
        for y in 0..terrain.height() {
            for x in 0..terrain.width() {
                if !is_low_deck_cell(terrain, (x, y)) {
                    continue;
                }
                let overlay = terrain
                    .cell(x, y)
                    .and_then(|c| c.bridge_layer.as_ref().map(|b| b.overlay_id))
                    .unwrap_or(0);
                low_by_overlay.entry(overlay).or_default().push((x, y));
            }
        }
        for (overlay, cells) in &low_by_overlay {
            let sample = cells[0];
            let pc = grid.cell(sample.0, sample.1);
            println!(
                "  low overlay {overlay}: {} cell(s), sample {sample:?} gw={:?} lvl={:?} \
                 water={:?}",
                cells.len(),
                pc.map(|c| c.ground_walkable),
                pc.map(|c| c.ground_level),
                terrain.cell(sample.0, sample.1).map(|c| c.is_water),
            );
        }
    }
}

/// One author-placed collapse gap: stamped deck on both sides, a hole between
/// them where the deck is missing and the ground is four levels down.
#[derive(Debug, Clone)]
struct CollapseGap {
    /// Off-bridge cell at deck level, one step before the near stub.
    approach: (u16, u16),
    /// Stamped stub cells on the near side, in travel order.
    near_stubs: Vec<(u16, u16)>,
    /// The hole: unstamped cells whose ground sits `BRIDGE_DECK_LEVEL_DELTA`
    /// below the stubs' deck.
    gap: Vec<(u16, u16)>,
    /// The first stamped cell on the far side of the hole.
    far_stub: (u16, u16),
    deck_level: u8,
    step: (i32, i32),
}

/// Find the longest author-placed collapse gap on a loaded map.
///
/// The pattern, read from `retail_damaged_bridge_inventory` output on
/// `Deadman.mmx` row `y=41`: stamped `(55,41)`/`(56,41)` with `deck_level = 4`
/// over `ground_level = 0`, then `(57..60,41)` unstamped at `ground_level = 0`
/// with `bridge_deck_level = 0` — the deck is simply absent — then a stamped
/// stub at `(61,41)` again at deck 4. `(56,41)` carries state byte 8 and
/// `(61,41)` state 7, the two `PartialCollapse` states the ledger names.
fn find_collapse_gap(grid: &PathGrid) -> Option<CollapseGap> {
    let mut best: Option<CollapseGap> = None;
    for y in 0..grid.height() {
        for x in 0..grid.width() {
            let Some(first) = grid.cell(x, y) else {
                continue;
            };
            if !first.bridge_structural {
                continue;
            }
            let deck_level = first.bridge_deck_level;
            let ground = first.ground_level;
            if i16::from(deck_level) != i16::from(ground) + BRIDGE_DECK_LEVEL_DELTA {
                continue;
            }
            for step in STEPS {
                // The cell behind must be a real off-bridge approach at deck level.
                let Some(approach) = offset((x, y), (-step.0, -step.1)) else {
                    continue;
                };
                let Some(cell_a) = grid.cell(approach.0, approach.1) else {
                    continue;
                };
                if cell_a.bridge_structural
                    || !cell_a.ground_walkable
                    || cell_a.ground_level != deck_level
                {
                    continue;
                }
                // Walk the stamped stubs.
                let mut near_stubs = vec![(x, y)];
                let mut cursor = (x, y);
                while let Some(next) = offset(cursor, step) {
                    match grid.cell(next.0, next.1) {
                        Some(cell) if cell.bridge_structural && cell.ground_level == ground => {
                            near_stubs.push(next);
                            cursor = next;
                        }
                        _ => break,
                    }
                }
                // Then the hole: unstamped, at the stubs' own ground level, i.e.
                // four levels below the deck the mover is standing on.
                let mut gap: Vec<(u16, u16)> = Vec::new();
                let mut probe = cursor;
                let far_stub = loop {
                    let Some(next) = offset(probe, step) else {
                        break None;
                    };
                    let Some(cell) = grid.cell(next.0, next.1) else {
                        break None;
                    };
                    if cell.bridge_structural {
                        break (cell.ground_level == ground
                            && cell.bridge_deck_level == deck_level)
                            .then_some(next);
                    }
                    if cell.ground_level != ground || gap.len() >= 8 {
                        break None;
                    }
                    gap.push(next);
                    probe = next;
                };
                let (Some(far_stub), false) = (far_stub, gap.is_empty()) else {
                    continue;
                };
                let candidate = CollapseGap {
                    approach,
                    near_stubs,
                    gap,
                    far_stub,
                    deck_level,
                    step,
                };
                if best
                    .as_ref()
                    .is_none_or(|current| candidate.gap.len() > current.gap.len())
                {
                    best = Some(candidate);
                }
            }
        }
    }
    best
}

/// Matrix row T2-09 — Drive, high **partially collapsed**, onto, player Move.
///
/// The row's own words: "Order across the gap. Correct behaviour is a refusal or
/// a route around, never a drive into the gap."
///
/// The mover is ordered from the near approach onto the stamped stub on the
/// **far** side of the hole. Every route to it along the bridge line has to pass
/// over cells where the deck is missing and the ground is four levels down.
///
/// What is asserted:
///
/// 1. The native height model holds on every frame, so a mover that did end up
///    in the hole would have to be at the riverbed level, not floating.
/// 2. No frame puts the mover on a gap cell carrying `on_bridge`, a
///    `BridgeOccupancy` entry, or deck height — that is "driving into the gap",
///    and it is the failure the row names.
/// 3. If the order was accepted and the mover arrived, it arrived by a route
///    that satisfies (2) — a legitimate way round.
///
/// A refusal is a **pass**, not a skip: for this row refusing is one of the two
/// correct answers, so the outcome is reported and both branches are checked.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_ordered_across_the_deadman_collapse_gap_never_drives_into_it() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let map_file = "Deadman.mmx";
    let mut scenario = match headless_scenario::load(&retail, map_file, SEED) {
        Ok(scenario) => scenario,
        Err(error) => panic!("load {map_file}: {error}"),
    };
    let gap = {
        let grid = scenario.sim().path_grid().expect("navigation published");
        find_collapse_gap(grid).unwrap_or_else(|| {
            panic!("{map_file} exposes no author-placed collapse gap; the fixture has changed")
        })
    };
    println!(
        "{map_file}: approach {:?} -> stubs {:?} -> GAP {:?} -> far stub {:?}, deck level {}, \
         step {:?}",
        gap.approach, gap.near_stubs, gap.gap, gap.far_stub, gap.deck_level, gap.step,
    );
    assert!(
        !gap.gap.is_empty(),
        "an empty gap is an intact span, not a collapse"
    );

    let owner_name = prepare_commanding_house(&mut scenario);
    let entity_id = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation
            .spawn_object(
                "MTNK",
                &owner_name,
                gap.approach.0,
                gap.approach.1,
                0,
                &resources.rules,
                &resources.height_map,
            )
            .unwrap_or_else(|| panic!("could not place an MTNK on {:?}", gap.approach))
    };
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    let owner_id = scenario
        .sim()
        .interner
        .get(&owner_name)
        .expect("owner interned");
    let execute_tick = scenario.sim().session.tick + 1;
    scenario
        .runtime
        .advance_frame(
            &[CommandEnvelope::new(
                owner_id,
                execute_tick,
                Command::Move {
                    entity_id,
                    target_rx: gap.far_stub.0,
                    target_ry: gap.far_stub.1,
                    queue: false,
                    group_id: None,
                },
            )],
            SIM_TICK_MS,
            TickLane::Ordinary,
        )
        .expect("fixture frame must complete");
    let accepted = scenario
        .sim()
        .entities()
        .get(entity_id)
        .and_then(|entity| entity.movement_target.as_ref())
        .map(|target| (target.path.len(), target.path.clone()));
    println!(
        "ordinary Command::Move {:?} -> {:?} (across the gap): {}",
        gap.approach,
        gap.far_stub,
        match &accepted {
            Some((len, path)) => format!("ACCEPTED, {len} node(s), {path:?}"),
            None => "REFUSED".to_string(),
        }
    );

    let rows = record_until(&mut scenario, entity_id, gap.far_stub);
    print_tick_table(&rows);

    // (1) The native height model, every frame.
    let violations: Vec<&TickRow> = rows.iter().filter(|row| !row.holds_invariant()).collect();
    assert!(
        violations.is_empty(),
        "position.z left the native model on {} frame(s); first {:?}",
        violations.len(),
        violations.first(),
    );

    // (2) The row's named failure: standing in the hole as if the deck were there.
    let gap_cells: std::collections::BTreeSet<(u16, u16)> = gap.gap.iter().copied().collect();
    let in_the_gap: Vec<&TickRow> = rows
        .iter()
        .filter(|row| gap_cells.contains(&row.cell))
        .collect();
    for row in &in_the_gap {
        assert!(
            !row.on_bridge,
            "the mover stood in the collapse gap at {:?} carrying on_bridge — it drove onto a \
             deck that is not there: {row:?}",
            row.cell
        );
        assert_eq!(
            row.occupancy_deck, None,
            "the mover took a BridgeOccupancy entry inside the collapse gap at {:?}: {row:?}",
            row.cell
        );
        assert_ne!(
            i16::from(row.z as i8),
            i16::from(row.terrain_level as i8) + BRIDGE_DECK_LEVEL_DELTA,
            "the mover sat at deck height inside the collapse gap at {:?}: {row:?}",
            row.cell
        );
    }

    let last = rows.last().copied().expect("frames recorded");
    let arrived = last.cell == gap.far_stub;
    println!(
        "\nT2-09: order {}; {} frame(s); {} frame(s) inside the gap (all at ground level, \
         off-bridge); last cell {:?}; arrived at the far stub: {arrived}",
        if accepted.is_some() {
            "ACCEPTED"
        } else {
            "REFUSED"
        },
        rows.len(),
        in_the_gap.len(),
        last.cell,
    );
    if arrived {
        // A route round is the other correct answer — but it has to be a route,
        // not a teleport through the hole.
        let deck_frames = rows.iter().filter(|row| row.structural).count();
        println!(
            "  arrived by a route with {deck_frames} stamped-cell frame(s) and \
             {} gap frame(s)",
            in_the_gap.len()
        );
    }
}

/// Matrix row T2-10 — Drive, **low damaged/destroyed**, along, player Move.
///
/// `Shrapnel.mmx` is the only loose map in the 184-map corpus with author-placed
/// non-pristine low-bridge overlay ids. Measured by
/// `retail_damaged_bridge_inventory`: overlays `100` (`0x64`) and `101` (`0x65`)
/// — the terminal sinks of the wood damage table — occupy three cells each, at
/// `(107,46)` and `(114,59)`, and **both sit over pre-overlay Water**.
///
/// So the row's question is sharp: a destroyed low bridge over water. Does VERA
/// still let a tank drive over it?
///
/// **Measured answer: no, and this is the right answer.** The ordinary Move is
/// admitted but its goal is resolved back to `(106,46)`, the last surviving deck
/// cell before the destroyed one; the tank drives out to the broken edge over
/// water and stops. It never occupies a `0x64`/`0x65` cell.
///
/// The discriminator is exact and worth stating, because a `ground_walkable`
/// read would have got it backwards — that flag is `true` on the destroyed cell,
/// the same trap the matrix's evidence gap 1 was opened over. `(106,46)` and
/// `(107,46)` sit on the *same* pre-overlay Water; the only difference between
/// them is the overlay id, and the tank crosses the first and cannot enter the
/// second. So passability here is decided by the damage-table overlay, not by
/// the water beneath.
///
/// What this does **not** claim: that the stopping *point* matches gamemd.
/// Whether the original refuses the order outright, truncates to the same cell,
/// or routes elsewhere is an unread question, as is whether `0x64`/`0x65` are
/// the right terminal sinks. Both need the binary.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_cannot_cross_a_destroyed_shrapnel_low_bridge() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    /// The terminal sinks of the wood low-bridge damage table.
    const DESTROYED_LOW_OVERLAYS: [u8; 2] = [0x64, 0x65];

    let map_file = "Shrapnel.mmx";
    let mut scenario = match headless_scenario::load(&retail, map_file, SEED) {
        Ok(scenario) => scenario,
        Err(error) => panic!("load {map_file}: {error}"),
    };
    let (span, destroyed) = {
        let sim = scenario.sim();
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("headless load keeps resolved terrain");
        let grid = sim.path_grid().expect("navigation published");
        let destroyed: std::collections::BTreeSet<(u16, u16)> = (0..terrain.height())
            .flat_map(|y| (0..terrain.width()).map(move |x| (x, y)))
            .filter(|(x, y)| {
                terrain.cell(*x, *y).is_some_and(|c| {
                    c.bridge_layer
                        .as_ref()
                        .is_some_and(|b| DESTROYED_LOW_OVERLAYS.contains(&b.overlay_id))
                })
            })
            .collect();
        assert!(
            !destroyed.is_empty(),
            "{map_file} carries no 0x64/0x65 low-bridge cell; the fixture has changed"
        );
        println!("destroyed low-bridge cells: {destroyed:?}");
        // The straight low run that contains one of them, longest first.
        let span = find_low_bridge_spans(terrain, grid)
            .into_iter()
            .find(|span| span.deck.iter().any(|cell| destroyed.contains(cell)))
            .unwrap_or_else(|| {
                panic!("no straight low run on {map_file} contains a destroyed cell")
            });
        print_low_inventory(terrain, grid, &span);
        (span, destroyed)
    };
    let on_route: Vec<(u16, u16)> = span
        .deck
        .iter()
        .copied()
        .filter(|cell| destroyed.contains(cell))
        .collect();
    println!(
        "chosen span {:?} -> {:?} ({} deck cell(s)); {} destroyed cell(s) on it: {on_route:?}; \
         water-backed deck cells: {:?}",
        span.approach_a,
        span.approach_b,
        span.deck.len(),
        on_route.len(),
        span.water_gap,
    );

    let owner_name = prepare_commanding_house(&mut scenario);
    let entity_id = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation
            .spawn_object(
                "MTNK",
                &owner_name,
                span.approach_a.0,
                span.approach_a.1,
                0,
                &resources.rules,
                &resources.height_map,
            )
            .unwrap_or_else(|| panic!("could not place an MTNK on {:?}", span.approach_a))
    };
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    let owner_id = scenario
        .sim()
        .interner
        .get(&owner_name)
        .expect("owner interned");
    let execute_tick = scenario.sim().session.tick + 1;
    scenario
        .runtime
        .advance_frame(
            &[CommandEnvelope::new(
                owner_id,
                execute_tick,
                Command::Move {
                    entity_id,
                    target_rx: span.approach_b.0,
                    target_ry: span.approach_b.1,
                    queue: false,
                    group_id: None,
                },
            )],
            SIM_TICK_MS,
            TickLane::Ordinary,
        )
        .expect("fixture frame must complete");
    let accepted = scenario
        .sim()
        .entities()
        .get(entity_id)
        .and_then(|entity| entity.movement_target.as_ref())
        .map(|target| target.path.len());
    println!("ordinary Command::Move across the destroyed strip: path={accepted:?}");

    let rows = record_until(&mut scenario, entity_id, span.approach_b);
    print_tick_table(&rows);

    let deck_frames: Vec<&TickRow> = rows
        .iter()
        .filter(|row| span.deck.contains(&row.cell))
        .collect();
    assert_low_span_invariant(&rows, &deck_frames);

    let destroyed_frames: Vec<&TickRow> = rows
        .iter()
        .filter(|row| destroyed.contains(&row.cell))
        .collect();
    let destroyed_visited: std::collections::BTreeSet<(u16, u16)> =
        destroyed_frames.iter().map(|row| row.cell).collect();
    let last = rows.last().copied().expect("frames recorded");
    println!(
        "\nT2-10: order {}; {} frame(s); {} frame(s) on a DESTROYED (0x64/0x65) cell over water, \
         covering {:?}; last cell {:?} (target {:?})",
        if accepted.is_some() {
            "ACCEPTED"
        } else {
            "REFUSED"
        },
        rows.len(),
        destroyed_frames.len(),
        destroyed_visited,
        last.cell,
        span.approach_b,
    );

    // 1. The destroyed cells are never entered — the whole point of the row.
    assert!(
        destroyed_frames.is_empty(),
        "the tank drove onto a destroyed (0x64/0x65) low-bridge cell over water: {:?}",
        destroyed_visited,
    );

    // 2. It did not simply fail to move: it reached the near edge of the break,
    //    which is the last surviving deck cell before the destroyed one. Without
    //    this the run would pass on a mover that never left its approach.
    let first_destroyed_on_route = *on_route.first().expect("a destroyed cell on the route");
    let near_edge = offset(first_destroyed_on_route, (-span.step.0, -span.step.1))
        .expect("the cell before the break is in bounds");
    assert_eq!(
        last.cell,
        near_edge,
        "the tank stopped at {:?} rather than at the near edge of the break {near_edge:?}; \
         cells visited {:?}",
        last.cell,
        rows.iter().map(|row| row.cell).collect::<Vec<_>>(),
    );

    // 3. The near edge and the destroyed cell sit on the same pre-overlay Water,
    //    so the overlay id — not the water — is what decides passability. This
    //    is what makes the run a bridge result rather than a water result.
    assert!(
        span.water_gap.contains(&near_edge) && span.water_gap.contains(&first_destroyed_on_route),
        "the near edge {near_edge:?} and the break {first_destroyed_on_route:?} are not both \
         water-backed, so this run does not isolate the damage overlay: {:?}",
        span.water_gap,
    );

    // 4. It did not get across.
    assert_ne!(
        last.cell, span.approach_b,
        "the tank completed the crossing over a destroyed low bridge"
    );
    println!(
        "T2-10: the break at {first_destroyed_on_route:?} is impassable; the tank drove to the \
         near edge {near_edge:?} — water-backed, same river, passable — and stopped."
    );
}

// ---------------------------------------------------------------------------
// UNDER an intact high span — matrix rows T1-13 / T1-14 / T1-15.
//
// VERA models a bridge as terrain on the *same cell* as the ground beneath it.
// There is no separate "under" cell: a mover is on the deck when `on_bridge` is
// set and it carries `ground + 4`, and under the span when it occupies the same
// cell with `on_bridge` clear at `ground`. So an "under" order is an ordinary
// ground-plane move whose route crosses the structural band sideways, i.e.
// along the river the span bridges rather than along the span.
//
// The geometry below is derived, never assumed: the drive line found by
// `find_high_bridge_span` is one lane of a wider structural band, and the
// transect walks perpendicular to it to find where the band ends and open
// valley floor resumes.
// ---------------------------------------------------------------------------

/// A perpendicular cut through the structural band at one deck cell: the route
/// a mover would take to pass *under* the span rather than along it.
#[derive(Debug, Clone)]
struct UnderSpanTransect {
    /// The drive-line deck cell the cut passes through.
    through: (u16, u16),
    /// Unit step across the band (90° from the span's own travel step).
    perp: (i32, i32),
    /// Every consecutive structural cell on the cut, in `perp` travel order.
    band: Vec<(u16, u16)>,
    /// Start of the under-span order: the first non-structural cell on the
    /// `-perp` side that still sits at the deck's own terrain level, backed off
    /// by `UNDER_APPROACH_MARGIN` so the mover takes a real run at the band.
    under_a: Option<(u16, u16)>,
    /// Same on the `+perp` side; the under-span order's destination.
    under_b: Option<(u16, u16)>,
    /// Terrain level under the span — the height an under-span mover must hold.
    deck_terrain_level: u8,
}

/// How far past the structural band the under-span order's endpoints sit. One
/// cell would leave the mover starting adjacent to the band, where the A*
/// blocked-goal tail (`core.rs`, "walk as close as you can") can end a run
/// before it proves anything.
const UNDER_APPROACH_MARGIN: i32 = 2;

/// Walk perpendicular to the span at `through` and record the structural band
/// plus the valley-floor cells either side of it.
fn build_under_span_transect(
    grid: &PathGrid,
    span: &HighBridgeSpan,
    through: (u16, u16),
) -> UnderSpanTransect {
    // The span travels along `span.step`; the cut is the 90° rotation of it.
    let perp = (span.step.1, span.step.0);
    let level = span.deck_terrain_level;

    let mut band = vec![through];
    // Extend backwards along `-perp` first so `band` reads in `perp` order.
    let mut back = Vec::new();
    let mut cursor = through;
    loop {
        let Some(next) = offset(cursor, (-perp.0, -perp.1)) else {
            break;
        };
        match grid.cell(next.0, next.1) {
            Some(cell) if cell.bridge_structural => {
                back.push(next);
                cursor = next;
            }
            _ => break,
        }
    }
    let band_start = cursor;
    back.reverse();
    back.extend(band.drain(..));
    band = back;
    let mut cursor = through;
    loop {
        let Some(next) = offset(cursor, perp) else {
            break;
        };
        match grid.cell(next.0, next.1) {
            Some(cell) if cell.bridge_structural => {
                band.push(next);
                cursor = next;
            }
            _ => break,
        }
    }
    let band_end = cursor;

    // The order's endpoints: `UNDER_APPROACH_MARGIN` cells beyond each end of
    // the band, accepted only when they are off the band, on the path grid, and
    // still at the deck's own terrain level — anything higher is the river bank,
    // not the space under the span.
    let endpoint = |from: (u16, u16), dir: (i32, i32)| -> Option<(u16, u16)> {
        let cell = offset(
            from,
            (dir.0 * UNDER_APPROACH_MARGIN, dir.1 * UNDER_APPROACH_MARGIN),
        )?;
        let facts = grid.cell(cell.0, cell.1)?;
        (!facts.bridge_structural && facts.ground_level == level).then_some(cell)
    };

    UnderSpanTransect {
        through,
        perp,
        band,
        under_a: endpoint(band_start, (-perp.0, -perp.1)),
        under_b: endpoint(band_end, perp),
        deck_terrain_level: level,
    }
}

/// Everything the passability instruments say about one cell, ground plane
/// first. Printed for every cell of a transect so the geometry section of the
/// report is a measurement rather than a summary.
#[allow(clippy::too_many_arguments)]
fn print_under_cell_row(
    label: &str,
    cell: (u16, u16),
    grid: &PathGrid,
    terrain: &ResolvedTerrainGrid,
    costs: &std::collections::BTreeMap<
        crate::rules::locomotor_type::SpeedType,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid,
    >,
    zones: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
) {
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::movement::locomotor::MovementLayer;

    let Some(pc) = grid.cell(cell.0, cell.1) else {
        println!("  {label} {cell:?}: NOT IN PATH GRID");
        return;
    };
    let Some(rt) = terrain.cell(cell.0, cell.1) else {
        println!("  {label} {cell:?}: NOT IN RESOLVED TERRAIN");
        return;
    };
    let cost = |st: SpeedType| costs.get(&st).map_or(255, |g| g.cost_at(cell.0, cell.1));
    let zone_of = |mz: MovementZone| {
        zones
            .and_then(|z| z.map_for(mz))
            .map(|m| {
                (
                    m.zone_at(cell.0, cell.1, MovementLayer::Ground),
                    m.zone_at(cell.0, cell.1, MovementLayer::Bridge),
                )
            })
            .map_or_else(|| "-/-".to_string(), |(g, b)| format!("{g}/{b}"))
    };
    println!(
        "  {label} {cell:?}: struct={} bwalk={} trans={} g_lvl={} deck_lvl={} gwalk={} \
         || under: water={} cliff={} gwblk={} base_gwblk={} land={} base_land={} yr_land={} \
         zone_type={} || cost Foot={} Track={} Wheel={} Amph={} Hover={} \
         || zone(G/B) Normal={} AmphDest={} Infantry={}",
        pc.bridge_structural,
        pc.bridge_walkable,
        pc.transition,
        pc.ground_level,
        pc.bridge_deck_level,
        pc.ground_walkable,
        rt.is_water,
        rt.is_cliff_like,
        rt.ground_walk_blocked,
        rt.base_ground_walk_blocked,
        rt.land_type,
        rt.base_land_type,
        rt.yr_cell_land_type,
        rt.zone_type,
        cost(SpeedType::Foot),
        cost(SpeedType::Track),
        cost(SpeedType::Wheel),
        cost(SpeedType::Amphibious),
        cost(SpeedType::Hover),
        zone_of(MovementZone::Normal),
        zone_of(MovementZone::AmphibiousDestroyer),
        zone_of(MovementZone::Infantry),
    );
}

/// Measurement only — matrix rows T1-13/14/15, step 1.
///
/// Evidence gap 1 was "measured" on 2026-08-28 by adding `ground_walkable` to
/// `print_inventory`, which reported 17/17 on BayOPigs and 22/22 on Hills. That
/// number is **not** a statement about the riverbed:
/// `PathGrid::from_resolved_terrain_with_bridges` (`sim/pathfinding/core.rs`)
/// hardcodes `ground_walkable = true` for any intact structural bridge cell,
/// with the comment "Intact bridge deck → walkable (overrides underlying
/// terrain)". `TerrainCostGrid::from_resolved_terrain` (`terrain_cost.rs`) does
/// the same, returning `COST_NORMAL` for every SpeedType on an elevated bridge
/// cell. Both flags therefore describe the deck, not the ground under it, on
/// every fixture — which is why this test prints the *underlying* terrain fields
/// those two overrides mask, and prints them for the valley floor either side of
/// the structural band as well.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_under_high_span_geometry() {
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::{AStarOptions, astar_search};

    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    for map_file in ["BayOPigs.mmx", "Hills.mmx"] {
        println!("\n===== {map_file} =====");
        let scenario = match headless_scenario::load(&retail, map_file, SEED) {
            Ok(scenario) => scenario,
            Err(error) => {
                println!("{map_file}: load failed: {error}");
                continue;
            }
        };
        let sim = scenario.sim();
        let grid = sim.path_grid().expect("navigation published");
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("headless load keeps resolved terrain");
        let Some(span) = find_high_bridge_span(grid) else {
            println!("{map_file}: no usable high span");
            continue;
        };
        println!(
            "drive line {:?} -> {:?}, {} deck cell(s), deck terrain level {}, approach level {}, \
             step {:?}",
            span.approach_a,
            span.approach_b,
            span.deck.len(),
            span.deck_terrain_level,
            span.approach_level,
            span.step,
        );

        // Structural footprint, whole-map. The drive line is one lane of it.
        let structural: Vec<(u16, u16)> = (0..grid.height())
            .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
            .filter(|(x, y)| grid.cell(*x, *y).is_some_and(|c| c.bridge_structural))
            .collect();
        println!(
            "{} structural cell(s) on the map; the discovered drive line accounts for {}",
            structural.len(),
            span.deck.len()
        );

        // Three cuts: the two ends of the span and its middle, so a report can
        // say whether the band width is uniform.
        let picks = [
            span.deck[0],
            span.deck[span.deck.len() / 2],
            *span.deck.last().expect("non-empty span"),
        ];
        for through in picks {
            let cut = build_under_span_transect(grid, &span, through);
            println!(
                "\ntransect through {:?} (perp {:?}): band {} cell(s) {:?} .. {:?}; \
                 under_a={:?} under_b={:?}",
                cut.through,
                cut.perp,
                cut.band.len(),
                cut.band.first(),
                cut.band.last(),
                cut.under_a,
                cut.under_b,
            );
            let mut cells: Vec<(&str, (u16, u16))> = Vec::new();
            for k in 1..=(UNDER_APPROACH_MARGIN + 2) {
                if let Some(c) = offset(
                    *cut.band.first().expect("non-empty band"),
                    (-cut.perp.0 * k, -cut.perp.1 * k),
                ) {
                    cells.push(("side", c));
                }
            }
            cells.reverse();
            for c in &cut.band {
                cells.push(("BAND", *c));
            }
            for k in 1..=(UNDER_APPROACH_MARGIN + 2) {
                if let Some(c) = offset(
                    *cut.band.last().expect("non-empty band"),
                    (cut.perp.0 * k, cut.perp.1 * k),
                ) {
                    cells.push(("side", c));
                }
            }
            for (label, c) in &cells {
                print_under_cell_row(
                    label,
                    *c,
                    grid,
                    terrain,
                    &sim.terrain_costs,
                    sim.zone_grid.as_ref(),
                );
            }

            // Which plane of each cell each mover may actually enter. This is
            // the production predicate (`is_cell_passable_for_category_on_layer`
            // → `evaluate_can_enter_cell`), not a flag read, and it is what
            // decides whether an under-span position exists at all.
            println!("  cell-entry admission by layer (G=Ground plane / B=deck plane):");
            for (label, c) in &cells {
                let mut per_mover = Vec::new();
                for (mover, zone, speed, infantry) in [
                    (
                        "MTNK",
                        MovementZone::Normal,
                        crate::rules::locomotor_type::SpeedType::Track,
                        false,
                    ),
                    (
                        "E1",
                        MovementZone::Infantry,
                        crate::rules::locomotor_type::SpeedType::Foot,
                        true,
                    ),
                    (
                        "ROBO",
                        MovementZone::AmphibiousDestroyer,
                        crate::rules::locomotor_type::SpeedType::Hover,
                        false,
                    ),
                ] {
                    let costs = sim.terrain_costs.get(&speed);
                    let probe = |layer| {
                        crate::sim::pathfinding::is_cell_passable_for_category_on_layer(
                            grid,
                            c.0,
                            c.1,
                            layer,
                            Some(zone),
                            None,
                            sim.resolved_terrain.as_ref(),
                            costs,
                            false,
                            crate::sim::pathfinding::cell_entry::TerrainEntryMode::AStarNeighbor,
                            infantry,
                            false,
                        )
                    };
                    per_mover.push(format!(
                        "{mover} G={} B={}",
                        probe(MovementLayer::Ground),
                        probe(MovementLayer::Bridge),
                    ));
                }
                println!("    {label} {c:?}: {}", per_mover.join(" | "));
            }

            // Ground-plane connectivity across the band, without any of the
            // production gates: does a layered search starting on Ground even
            // produce a route, and on which layer does it cross the band?
            if let (Some(a), Some(b)) = (cut.under_a, cut.under_b) {
                for (label, zone, speed) in [
                    (
                        "MTNK/Normal/Track",
                        MovementZone::Normal,
                        crate::rules::locomotor_type::SpeedType::Track,
                    ),
                    (
                        "E1/Infantry/Foot",
                        MovementZone::Infantry,
                        crate::rules::locomotor_type::SpeedType::Foot,
                    ),
                    (
                        "ROBO/AmphDest/Hover",
                        MovementZone::AmphibiousDestroyer,
                        crate::rules::locomotor_type::SpeedType::Hover,
                    ),
                ] {
                    let options = AStarOptions {
                        terrain_costs: sim.terrain_costs.get(&speed),
                        movement_zone: Some(zone),
                        resolved_terrain: sim.resolved_terrain.as_ref(),
                        ..Default::default()
                    };
                    match astar_search(grid, a, MovementLayer::Ground, b, &options) {
                        Some(steps) => {
                            // Any structural cell, not just this transect's four:
                            // the band runs the whole length of the span, so a
                            // route that travels along it sideways touches
                            // structural cells at other span positions.
                            let structural_steps = steps
                                .iter()
                                .filter(|s| {
                                    grid.cell(s.rx, s.ry).is_some_and(|c| c.bridge_structural)
                                })
                                .map(|s| ((s.rx, s.ry), s.layer))
                                .collect::<Vec<_>>();
                            println!(
                                "  ground-plane A* [{label}] {a:?} -> {b:?}: {} step(s); \
                                 {} on structural cells: {structural_steps:?}",
                                steps.len(),
                                structural_steps.len(),
                            );
                            println!(
                                "    route: {:?}",
                                steps
                                    .iter()
                                    .map(|s| (
                                        (s.rx, s.ry),
                                        match s.layer {
                                            MovementLayer::Bridge => 'B',
                                            _ => 'g',
                                        }
                                    ))
                                    .collect::<Vec<_>>()
                            );
                        }
                        None => println!("  ground-plane A* [{label}] {a:?} -> {b:?}: NO PATH"),
                    }
                }
            } else {
                println!(
                    "  no valley-floor endpoints either side of the band; no under order is expressible here"
                );
            }
        }
    }
}

/// One recorded under-span run.
struct UnderSpanRun {
    span: HighBridgeSpan,
    cut: UnderSpanTransect,
    start_cell: (u16, u16),
    order_accepted: bool,
    rows: Vec<TickRow>,
}

impl UnderSpanRun {
    /// Frames on a stamped bridge cell at deck height — the mover went *over*.
    fn deck_frames(&self) -> Vec<&TickRow> {
        self.rows
            .iter()
            .filter(|row| row.structural && row.on_bridge)
            .collect()
    }

    /// Frames on a stamped bridge cell with `on_bridge` clear — the mover is
    /// standing on the ground plane of a cell a span passes over, which is what
    /// "under the span" means in VERA's same-cell bridge model.
    fn under_frames(&self) -> Vec<&TickRow> {
        self.rows
            .iter()
            .filter(|row| row.structural && !row.on_bridge)
            .collect()
    }
}

/// The under-span invariant: `ObjectClass::GetHeight` @ `0x005F5F30` with
/// OnBridge clear, plus the two other places the deck term is stored.
///
/// A mover under a span occupies the same cell as the deck above it, so the only
/// thing separating the two states is this triple: no `on_bridge` flag, no
/// `BridgeOccupancy` entry, and `position.z` at the cell's own ground level
/// rather than `ground + 4`. All three are asserted, because any one alone would
/// pass an implementation that got the other two wrong.
fn assert_under_span_invariant(frames: &[&TickRow]) {
    for row in frames {
        assert!(
            !row.on_bridge,
            "under-span frame on {:?} carries on_bridge: {row:?}",
            row.cell
        );
        assert_eq!(
            row.occupancy_deck, None,
            "under-span frame on {:?} produced a BridgeOccupancy entry: {row:?}",
            row.cell
        );
        assert_eq!(
            i16::from(row.z as i8),
            i16::from(row.terrain_level as i8),
            "under-span frame on {:?} is not at the cell's own ground level: {row:?}",
            row.cell
        );
        assert_ne!(
            i16::from(row.z as i8),
            i16::from(row.terrain_level as i8) + BRIDGE_DECK_LEVEL_DELTA,
            "under-span frame on {:?} sits at deck height: {row:?}",
            row.cell
        );
    }
}

/// Record every committed frame until the mover reaches `goal`, goes idle, or
/// `MAX_TICKS` elapses. Shared by the under-span drivers.
fn record_until(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    entity_id: u64,
    goal: (u16, u16),
) -> Vec<TickRow> {
    let mut rows: Vec<TickRow> = Vec::new();
    let mut idle_frames = 0u32;
    for _ in 0..MAX_TICKS {
        scenario.tick();
        let sim = scenario.sim();
        let Some(entity) = sim.entities().get(entity_id) else {
            panic!("the mover vanished mid-run");
        };
        let cell = (entity.position.rx, entity.position.ry);
        let Some(facts) = sim
            .path_grid()
            .and_then(|grid| grid.cell(cell.0, cell.1))
            .copied()
        else {
            panic!("the mover left the path grid at {cell:?}");
        };
        let loco_layer = entity.movement_layer_or_ground();
        if entity.category == crate::map::entities::EntityCategory::Infantry
            && facts.bridge_structural
            && !entity.on_bridge
        {
            use crate::sim::movement::locomotor::MovementLayer;
            let occupation = sim
                .occupancy()
                .get(cell.0, cell.1)
                .expect("infantry has a live cell list");
            let slot = occupation
                .infantry(MovementLayer::Ground)
                .find_map(|(id, slot)| (id == entity_id).then_some(slot))
                .expect("under-span infantry must occupy a ground subcell");
            assert!(super::bump_crush::FUNCTIONAL_SUB_CELLS.contains(&slot));
            assert!(
                !occupation
                    .infantry(MovementLayer::Bridge)
                    .any(|(id, _)| id == entity_id)
            );
            assert_eq!(loco_layer, MovementLayer::Ground);
            let world_xy = super::ground_pose::position_world_xy(&entity.position);
            assert_eq!(
                entity.position.exact_z_leptons,
                Some(
                    crate::util::lepton::ground_height_leptons(
                        facts.ground_level,
                        facts.slope_type,
                        world_xy[0],
                        world_xy[1]
                    )
                    .expect("retail ground slope")
                ),
                "Walk must keep the ground surface Z under the span"
            );
        }
        rows.push(TickRow {
            tick: sim.session.tick,
            cell,
            z: entity.position.z,
            on_bridge: entity.on_bridge,
            occupancy_deck: entity.bridge_occupancy.map(|occ| occ.deck_level),
            terrain_level: facts.ground_level,
            structural: facts.bridge_structural,
            bridge_walkable: facts.bridge_walkable,
            transition: facts.transition,
            low_tube: facts.low_bridge_tube_cell,
            stored_deck_level: facts.bridge_deck_level,
            loco_layer,
            prefix_z: facts.effective_cell_z_for_layer(loco_layer),
        });
        if entity.movement_target.is_none() {
            idle_frames += 1;
            if idle_frames >= 30 {
                break;
            }
        } else {
            idle_frames = 0;
        }
        if cell == goal {
            break;
        }
    }
    rows
}

/// Spawn `unit_type` on the valley floor beside a high span and order it, with
/// an ordinary undisabled `Command::Move`, to the matching cell on the other
/// side — a route whose straight line passes beneath the deck.
///
/// Returns `None` when the mover cannot be placed on the valley floor at all,
/// which is itself the answer for that locomotor on that fixture.
fn order_under_high_span(map_file: &str, unit_type: &str) -> Option<UnderSpanRun> {
    let retail = retail_dir()?;
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let mut scenario = match headless_scenario::load(&retail, map_file, SEED) {
        Ok(scenario) => scenario,
        Err(error) => panic!("load {map_file}: {error}"),
    };

    let (span, cut) = {
        let grid = scenario.sim().path_grid().expect("navigation published");
        let span = find_high_bridge_span(grid)
            .unwrap_or_else(|| panic!("{map_file} exposes no high-bridge span"));
        let through = span.deck[span.deck.len() / 2];
        let cut = build_under_span_transect(grid, &span, through);
        (span, cut)
    };
    println!(
        "{map_file}: span {:?}..{:?} (terrain level {}), band {:?}..{:?}, under_a={:?} under_b={:?}",
        span.deck.first(),
        span.deck.last(),
        cut.deck_terrain_level,
        cut.band.first(),
        cut.band.last(),
        cut.under_a,
        cut.under_b,
    );
    let (Some(under_a), Some(under_b)) = (cut.under_a, cut.under_b) else {
        println!(
            "{map_file}: no valley-floor cell at terrain level {} on both sides of the band; \
             an under-span order is not expressible on this geometry",
            cut.deck_terrain_level
        );
        return None;
    };

    let owner_name = prepare_commanding_house(&mut scenario);
    let mut entity_id = None;
    let mut start_cell = under_a;
    for candidate in [
        under_a,
        offset(under_a, (-cut.perp.0, -cut.perp.1)).unwrap_or(under_a),
    ] {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        if let Some(id) = simulation.spawn_object(
            unit_type,
            &owner_name,
            candidate.0,
            candidate.1,
            0,
            &resources.rules,
            &resources.height_map,
        ) {
            entity_id = Some(id);
            start_cell = candidate;
            break;
        }
    }
    let Some(entity_id) = entity_id else {
        println!(
            "{map_file}: a {unit_type} cannot be placed on the valley floor at {under_a:?}; \
             this locomotor cannot reach the underside of this span at all"
        );
        return None;
    };
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    {
        let entity = scenario
            .sim()
            .entities()
            .get(entity_id)
            .expect("spawned mover present");
        println!(
            "spawned {unit_type} id={entity_id} at {start_cell:?} z={} on_bridge={}",
            entity.position.z, entity.on_bridge
        );
    }

    // Same order entry as the crossing drivers: a `Command::Move` envelope
    // through the ordinary tick lane, nothing disabled.
    let owner_id = scenario
        .sim()
        .interner
        .get(&owner_name)
        .expect("owner interned");
    let execute_tick = scenario.sim().session.tick + 1;
    scenario
        .runtime
        .advance_frame(
            &[CommandEnvelope::new(
                owner_id,
                execute_tick,
                Command::Move {
                    entity_id,
                    target_rx: under_b.0,
                    target_ry: under_b.1,
                    queue: false,
                    group_id: None,
                },
            )],
            SIM_TICK_MS,
            TickLane::Ordinary,
        )
        .expect("fixture frame must complete");
    let path = scenario
        .sim()
        .entities()
        .get(entity_id)
        .and_then(|entity| entity.movement_target.as_ref())
        .map(|target| target.path.len());
    println!("ordinary Command::Move {start_cell:?} -> {under_b:?}: path={path:?}");
    if path.is_none() {
        diagnose_rejected_order(
            &mut scenario,
            &owner_name,
            entity_id,
            start_cell,
            under_a,
            under_b,
            &cut.band,
        );
    }

    let rows = if path.is_some() {
        record_until(&mut scenario, entity_id, under_b)
    } else {
        Vec::new()
    };
    print_tick_table(&rows);
    Some(UnderSpanRun {
        span,
        cut,
        start_cell,
        order_accepted: path.is_some(),
        rows,
    })
}

/// Report an under-span run and hold it to whichever outcome it produced.
///
/// The assertion set is deliberately two-sided. If a run ever *does* put the
/// mover on the ground plane of a stamped cell, the under-span invariant is
/// checked on those frames — a positive result must still be correct, not just
/// present. The characterization below only fires when it does not.
fn judge_under_span_run(run: &UnderSpanRun, unit_type: &str, map_file: &str) {
    let under = run.under_frames();
    let deck = run.deck_frames();
    // A positive result is checked before it is counted.
    assert_under_span_invariant(&under);
    let visited: std::collections::BTreeSet<(u16, u16)> =
        run.rows.iter().map(|row| row.cell).collect();
    println!(
        "\n{map_file}/{unit_type}: span {:?}..{:?} at terrain level {}, band {} cell(s) wide; \
         {} frame(s) from {:?}; {} under-span frame(s), {} deck frame(s); \
         visited {} distinct cell(s), last {:?}",
        run.span.deck.first(),
        run.span.deck.last(),
        run.span.deck_terrain_level,
        run.cut.band.len(),
        run.rows.len(),
        run.start_cell,
        under.len(),
        deck.len(),
        visited.len(),
        run.rows.last().map(|row| row.cell),
    );
    if !under.is_empty() {
        println!(
            "UNDER-SPAN OBSERVED on {:?}",
            under
                .iter()
                .map(|row| row.cell)
                .collect::<std::collections::BTreeSet<_>>()
        );
    }
    // Every frame, under or over, still obeys the native height model.
    let violations: Vec<&TickRow> = run
        .rows
        .iter()
        .filter(|row| !row.holds_invariant())
        .collect();
    assert!(
        violations.is_empty(),
        "position.z left the native model on {} frame(s); first: {:?}",
        violations.len(),
        violations.first(),
    );
}

/// Matrix row T1-13 — Drive **under** an intact high span, `Hills.mmx`.
///
/// Until 2026-09-15 this row pinned that an ordinary undisabled `Command::Move`
/// from one side of the span's footprint to the other was **refused outright**.
/// The diagnosis below is kept because it names the mechanism that was acting;
/// the settlement paragraph at the end names the native contract that retired it.
///
/// Measured on `Hills.mmx`, order `(87,71)` → `(87,78)`, valley floor at terrain
/// level 2 with the deck at 6:
///
/// * The terrain under the span is **ordinary passable land**, not a chasm:
///   `is_water=false`, `is_cliff_like=false`, `ground_walk_blocked=false`,
///   `zone_type=0`, `Track` cost 100, and Normal ground zone **2** — the same
///   zone as `(87,71)` and `(87,78)` either side. So this row is not excluded
///   by geometry.
/// * The structural band is **four** cells wide (`y = 73..76`) at every span
///   position, not the one-cell drive line the ledger names.
/// * The straight 8-node ground-plane route exists and is found by the flat A*,
///   by the layered A*, and by `find_move_path` with the zone grid, the terrain
///   cost grid, resolved terrain, playfield bounds and entity blocks all
///   supplied. It is lost by exactly one input: dropping
///   `blocker_neighbor_counts` from the production argument set restores it
///   (`find_move_path[production] -> None`,
///   `find_move_path[production - neighbors] -> Some(8)`), and that flag is
///   what selects the hierarchy branch of `find_layered_path_zoned_marker`
///   (`sim/pathfinding/zone_search.rs`).
/// * Two things fire on that branch, and both are real.
///   (b) The hierarchy branch is the only one that forwards `movement_zone`
///   into the cell-entry predicate, and with a speed type present
///   `evaluate_shared_cell_leaf` (`sim/pathfinding/cell_entry.rs`) stops
///   short-circuiting and reaches `evaluate_is_clear_to_move`'s
///   `has_bridge && !is_bridge → LevelMismatch` (`sim/cell_rect.rs`,
///   `CellClass::CheckCellPassability` @ `0x004834A0`). **(b) alone is
///   sufficient:** `retail_under_high_span_geometry` runs a bare `astar_search`
///   with terrain costs, resolved terrain and `movement_zone` supplied and *no*
///   hierarchy gate at all, and it already abandons the 8-node straight route
///   for a 92-step detour that climbs the east bridgehead, drives the span's
///   `y = 76` lane on the Bridge layer and comes back round.
///   (a) The hierarchy gate then removes even that detour — production returns
///   no path at all. Its deck exemption in `core.rs`
///   (`!neighbor_is_bridge_deck`) is keyed on the mover already carrying deck
///   height, so an under-span step never receives it. The ablation cannot
///   isolate (a) on its own, because dropping `blocker_neighbor_counts` also
///   drops the `movement_zone` forwarding that causes (b).
/// * Ordering *to* a cell under the span is separately impossible:
///   `astar_search` resolves the goal height of any bridge-passable goal cell to
///   `bridge_deck_level`, so a Move naming a stamped cell always aims at the
///   deck. `deck_and_ground_under_one_high_bridge_cell_are_separate_occupancy_planes`
///   shows that arm working as designed.
///
/// **Settled 2026-09-15 as a positive crossing.** `UnitClass::Can_Enter_Cell`
/// @ `0x0073F0A0` never calls `CheckCellPassability` @ `0x004834A0` (its callers
/// are CellRect, threat scans, placement, paradrop, overlay Mark and Jumpjet
/// touchdown); it clears its deck flag for a path height within one of the cell
/// level (`0x0073F0B7..F0E8`), defers height legality to `CheckBridgeTraversal`
/// @ `0x004D9C60` whose equal-level arm admits, walks the ground list `+0xE4`
/// (`0x0073F51A`) and reads the terrain's own land row at `0x0073FAB5`. The
/// Rust class arm in `cell_entry.rs` now covers every ground-layer mover, and
/// the cost grid's ground row replaces the deck override beneath a span.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_ordered_under_hills_high_bridge_crosses_all_four_ground_lanes() {
    let Some(run) = order_under_high_span("Hills.mmx", "MTNK") else {
        return;
    };
    judge_under_span_run(&run, "MTNK", "Hills.mmx");
    assert_eq!(run.start_cell, (87, 71));
    assert_eq!(run.cut.under_b, Some((87, 78)));
    assert_eq!(run.cut.band, vec![(87, 73), (87, 74), (87, 75), (87, 76)]);
    assert!(
        run.order_accepted,
        "ordinary Move beneath the span must be accepted"
    );
    assert_eq!(run.rows.last().map(|row| row.cell), run.cut.under_b);
    assert!(
        run.deck_frames().is_empty(),
        "the route must stay beneath the deck"
    );
    let under_cells: std::collections::BTreeSet<_> =
        run.under_frames().iter().map(|row| row.cell).collect();
    for cell in &run.cut.band {
        assert!(
            under_cells.contains(cell),
            "under-span route skipped lane {cell:?}"
        );
    }
}

/// Matrix row T1-14 — Walk under an intact high span, `Hills.mmx`.
///
/// Ordinary Move uses the production hierarchy, terrain and occupation inputs.
/// Native Infantry +0x1AC @ 0x0051BF90 admits ground beneath structural cells
/// after +0x1B0 @ 0x004D9C60; it never applies the unrelated 0x004834A0 level
/// rejection. The user also observed this crossing in original YR. This is a
/// Rust movement regression, not a native pixel or full path-sequence golden.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn infantry_ordered_under_hills_high_bridge_crosses_all_four_ground_lanes() {
    if retail_dir().is_none() {
        return;
    }
    let run =
        order_under_high_span("Hills.mmx", "E1").expect("E1 must spawn beside the Hills span");
    judge_under_span_run(&run, "E1", "Hills.mmx");
    assert_eq!(run.start_cell, (87, 71));
    assert_eq!(run.cut.under_b, Some((87, 78)));
    assert_eq!(run.cut.band, vec![(87, 73), (87, 74), (87, 75), (87, 76)]);
    assert!(
        run.order_accepted,
        "ordinary Move beneath the span must be accepted"
    );
    assert_eq!(run.rows.last().map(|row| row.cell), run.cut.under_b);
    assert!(
        run.deck_frames().is_empty(),
        "the route must stay beneath the deck"
    );
    let under_cells: std::collections::BTreeSet<_> =
        run.under_frames().iter().map(|row| row.cell).collect();
    for cell in &run.cut.band {
        assert!(
            under_cells.contains(cell),
            "under-span route skipped lane {cell:?}"
        );
    }
}

/// Matrix row T1-15 — Hover under an intact high span, `BayOPigs.mmx`.
///
/// `ROBO` is `AmphibiousDestroyer`, so the riverbed either side of this span is
/// water it can swim; this is the one fixture/locomotor pair where the valley
/// floor beside a water-backed span is reachable at all. Measured on the
/// `x = 111` span at `y = 143`:
///
/// * The riverbed beside the band **is** passable to it — `(106..109, 143)` and
///   `(114..117, 143)` are water at terrain level 1, `Hover` cost 100, ground
///   zone 2, and the cell-entry probe returns Ground=true for `ROBO` on every
///   one. It returns Ground=false for `MTNK` and `E1` there, which is why
///   T1-13/T1-14 have no BayOPigs arm — see
///   `tank_cannot_reach_the_riverbed_beside_bay_of_pigs_high_bridge`.
/// * **This corrects a recorded VERIFIED claim.** The reachability diagnosis's
///   residual R5 says an under-span route is "impossible on Bay of Pigs, whose
///   riverbed is zone 0". The riverbed is zone_type 4 (Water) and its Normal
///   ground zone is 0, but its `AmphibiousDestroyer` ground zone is 2 — a real,
///   connected zone. Zone 0 is a per-`MovementZone` answer, not a property of
///   the cell.
/// * The band is again four cells wide, `x = 110..113`, and the ordinary Move
///   across it is refused for the same reason as Hills.
///
/// **Settled 2026-09-15 as a positive crossing** on the same evidence as the
/// Hills tank run: the Unit entry contract admits the riverbed beneath the span
/// for a mover whose own land row is open there.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn hover_tank_ordered_under_bay_of_pigs_high_bridge_crosses_all_four_ground_lanes() {
    let Some(run) = order_under_high_span("BayOPigs.mmx", "ROBO") else {
        return;
    };
    judge_under_span_run(&run, "ROBO", "BayOPigs.mmx");
    assert_eq!(run.start_cell, (108, 143));
    assert_eq!(run.cut.under_b, Some((115, 143)));
    assert_eq!(
        run.cut.band,
        vec![(110, 143), (111, 143), (112, 143), (113, 143)]
    );
    assert!(
        run.order_accepted,
        "ordinary Move beneath the span must be accepted"
    );
    assert_eq!(run.rows.last().map(|row| row.cell), run.cut.under_b);
    assert!(
        run.deck_frames().is_empty(),
        "the route must stay beneath the deck"
    );
    let under_cells: std::collections::BTreeSet<_> =
        run.under_frames().iter().map(|row| row.cell).collect();
    for cell in &run.cut.band {
        assert!(
            under_cells.contains(cell),
            "under-span route skipped lane {cell:?}"
        );
    }
}

/// Ground-plane admission beneath a span is decided by the mover's own land row,
/// not by the lane's `bridge_transition` flag.
///
/// Until 2026-09-15 supplying a `MovementZone` (hence a speed type) sealed the
/// one band lane whose `bridge_transition` flag is clear: `evaluate_shared_cell_leaf`
/// (`sim/pathfinding/cell_entry.rs`) fell through to `evaluate_is_clear_to_move`
/// (`sim/cell_rect.rs`), whose `has_bridge && !is_bridge → LevelMismatch` arm
/// refused the ground plane of any `0x100` cell, while the three transition
/// lanes escaped through the short-circuit above it. That leaf ports
/// `CellClass::CheckCellPassability` @ `0x004834A0`, which neither Foot
/// `+0x1AC` implementation calls (`0x0051BF90`, `0x0073F0A0`); the class arm
/// now covers every ground-layer mover and all four lanes agree.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn ground_entry_under_a_high_span_is_admitted_on_every_lane() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::cell_entry::TerrainEntryMode;

    let scenario = headless_scenario::load(&retail, "Hills.mmx", SEED).expect("Hills.mmx loads");
    let sim = scenario.sim();
    let grid = sim.path_grid().expect("navigation published");
    let span = find_high_bridge_span(grid).expect("Hills exposes a high span");
    let cut = build_under_span_transect(grid, &span, span.deck[span.deck.len() / 2]);
    let costs = sim.terrain_costs.get(&SpeedType::Track);
    let probe = |cell: (u16, u16), zone: Option<MovementZone>| {
        crate::sim::pathfinding::is_cell_passable_for_category_on_layer(
            grid,
            cell.0,
            cell.1,
            MovementLayer::Ground,
            zone,
            None,
            sim.resolved_terrain.as_ref(),
            costs,
            false,
            TerrainEntryMode::AStarNeighbor,
            false,
            false,
        )
    };
    let mut sealed = Vec::new();
    let mut open = Vec::new();
    for cell in &cut.band {
        let facts = grid.cell(cell.0, cell.1).expect("band cell in the grid");
        let with_zone = probe(*cell, Some(MovementZone::Normal));
        let without_zone = probe(*cell, None);
        println!(
            "band {cell:?} transition={} : Ground entry with MovementZone::Normal = {with_zone}, \
             with no movement zone = {without_zone}",
            facts.transition,
        );
        assert!(
            without_zone,
            "band cell {cell:?} refuses ground entry even with no speed type; the mechanism this \
             test isolates is not the one acting"
        );
        if with_zone {
            open.push((*cell, facts.transition));
        } else {
            sealed.push((*cell, facts.transition));
        }
    }
    println!("open lanes {open:?}; sealed lanes {sealed:?}");
    // Settled 2026-09-15: the Unit entry contract (`0x0073F0A0`) never reaches
    // the `0x004834A0` level rejection, so every lane of the band — transition
    // or not — admits ground entry with a speed type supplied.
    assert!(
        sealed.is_empty(),
        "a band lane refused ground entry with a speed type supplied: {sealed:?}; the retired \
         Cell-leaf level rejection is acting again"
    );
    assert!(
        open.iter().any(|(_, transition)| !*transition),
        "the band carries no non-transition lane, so this test no longer isolates anything: \
         {open:?}"
    );
}

/// The T1-13 arm on `BayOPigs.mmx`: an `MTNK` cannot be placed on, or ordered
/// along, the riverbed beside this span at all.
///
/// Kept separate from the Hills arm because it answers a different question. On
/// Hills the terrain under and beside the span is ordinary land and only the
/// bridge stamp stops the mover; here the riverbed is water with `Track` cost 0,
/// so a Drive mover is excluded by terrain before the bridge is reached. Both
/// are NOT-APPLICABLE for the row, for different reasons, and the distinction is
/// what stops a later reader concluding that water is why under-span movement
/// never works.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn tank_cannot_reach_the_riverbed_beside_bay_of_pigs_high_bridge() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::cell_entry::TerrainEntryMode;

    let scenario =
        headless_scenario::load(&retail, "BayOPigs.mmx", SEED).expect("BayOPigs.mmx loads");
    let sim = scenario.sim();
    let grid = sim.path_grid().expect("navigation published");
    let span = find_high_bridge_span(grid).expect("BayOPigs exposes a high span");
    let cut = build_under_span_transect(grid, &span, span.deck[span.deck.len() / 2]);
    let (under_a, under_b) = (
        cut.under_a.expect("west riverbed cell"),
        cut.under_b.expect("east riverbed cell"),
    );
    let track = sim.terrain_costs.get(&SpeedType::Track);
    for cell in [under_a, under_b] {
        let admitted = crate::sim::pathfinding::is_cell_passable_for_category_on_layer(
            grid,
            cell.0,
            cell.1,
            MovementLayer::Ground,
            Some(MovementZone::Normal),
            None,
            sim.resolved_terrain.as_ref(),
            track,
            false,
            TerrainEntryMode::AStarNeighbor,
            false,
            false,
        );
        let rt = sim
            .resolved_terrain
            .as_ref()
            .and_then(|t| t.cell(cell.0, cell.1))
            .expect("riverbed cell resolved");
        println!(
            "riverbed {cell:?}: Track cost {} is_water={} zone_type={} -> Ground entry {admitted}",
            track.map_or(255, |g| g.cost_at(cell.0, cell.1)),
            rt.is_water,
            rt.zone_type,
        );
        assert!(
            !admitted,
            "the riverbed at {cell:?} admits a Normal/Track mover on the ground plane; \
             T1-13 now has a BayOPigs arm and this test must be replaced by a real run"
        );
    }
}

/// Questions 2 and 3 together: what the under-span state actually looks like,
/// and whether the deck and the ground under it can be occupied at once.
///
/// The ordinary-order tests above show no *order* can put a mover beneath a
/// span. This one puts one there directly and then drives it out with an
/// ordinary undisabled `Command::Move`, which is the only way currently
/// available to observe the state at all.
///
/// **What it rides on, stated rather than hidden:** the mover reaches the
/// under-span position through `spawn_object`, which the ledger records as
/// KNOWN-BROKEN (`N-01`: no bridge-deck term, so a spawn on a stamped cell
/// lands at the riverbed height). That defect is exactly what makes the
/// observation possible, so this test **cannot** settle T1-13/14/15 under the
/// program's rule 2 — the order is ordinary but the starting position is not
/// something a player can produce. It is recorded because it answers the two
/// sub-questions the rows ask, and because it pins the layered-occupancy
/// behaviour that nothing else in the tree exercises.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn deck_and_ground_under_one_high_bridge_cell_are_separate_occupancy_planes() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    let _ = env_logger::builder()
        .is_test(false)
        .filter_level(log::LevelFilter::Warn)
        .try_init();

    let mut scenario =
        headless_scenario::load(&retail, "Hills.mmx", SEED).expect("Hills.mmx loads");
    let (span, cut) = {
        let grid = scenario.sim().path_grid().expect("navigation published");
        let span = find_high_bridge_span(grid).expect("Hills exposes a high span");
        let through = span.deck[span.deck.len() / 2];
        let cut = build_under_span_transect(grid, &span, through);
        (span, cut)
    };
    // The drive line cell the deck mover will stop on, and the same cell's
    // ground plane for the under mover.
    let shared = cut.through;
    let exit = cut.under_b.expect("south valley cell");
    println!(
        "shared cell {shared:?} (terrain level {}, deck level {}); deck mover enters from {:?}, \
         under mover leaves to {exit:?}",
        cut.deck_terrain_level,
        cut.deck_terrain_level + BRIDGE_DECK_LEVEL_DELTA as u8,
        span.approach_a,
    );

    let owner_name = prepare_commanding_house(&mut scenario);

    // 1. Deck mover: ordinary Move from the west approach to the mid-span cell.
    //    `astar_search` resolves a bridge-passable goal to `bridge_deck_level`,
    //    so this is the order that parks a unit on the deck.
    let deck_id = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation
            .spawn_object(
                "MTNK",
                &owner_name,
                span.approach_a.0,
                span.approach_a.1,
                0,
                &resources.rules,
                &resources.height_map,
            )
            .expect("MTNK placed on the west approach")
    };
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    assert!(
        issue_ordinary_move(&mut scenario, &owner_name, deck_id, shared),
        "the ordinary Move onto the deck was refused"
    );
    let deck_rows = record_until(&mut scenario, deck_id, shared);
    let deck_last = *deck_rows.last().expect("deck mover recorded frames");
    println!(
        "deck mover finished at {:?} z={} on_bridge={} occ={:?}",
        deck_last.cell, deck_last.z, deck_last.on_bridge, deck_last.occupancy_deck
    );
    assert_eq!(
        deck_last.cell, shared,
        "the deck mover did not reach the shared cell"
    );
    assert!(deck_last.on_bridge, "the deck mover is not on the deck");
    assert_eq!(
        i16::from(deck_last.z as i8),
        i16::from(deck_last.terrain_level as i8) + BRIDGE_DECK_LEVEL_DELTA,
        "the deck mover is not at deck height: {deck_last:?}"
    );

    // 2. Under mover: placed on the ground plane of the same cell.
    let under_id = {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.spawn_object(
            "MTNK",
            &owner_name,
            shared.0,
            shared.1,
            0,
            &resources.rules,
            &resources.height_map,
        )
    };
    let Some(under_id) = under_id else {
        println!(
            "a second MTNK could NOT be placed on {shared:?} while the deck above it is \
             occupied — the two planes are not independently occupiable through spawn"
        );
        return;
    };
    let (under_z, under_on_bridge, under_occ) = {
        let entity = scenario
            .sim()
            .entities()
            .get(under_id)
            .expect("under mover present");
        (
            entity.position.z,
            entity.on_bridge,
            entity.bridge_occupancy.map(|occ| occ.deck_level),
        )
    };
    println!(
        "under mover placed on {shared:?}: z={under_z} on_bridge={under_on_bridge} occ={under_occ:?}"
    );

    // Both entities exist, on one cell, at two heights.
    let deck_still = scenario
        .sim()
        .entities()
        .get(deck_id)
        .expect("deck mover still present");
    assert_eq!(
        (deck_still.position.rx, deck_still.position.ry),
        shared,
        "placing a mover under the deck displaced the mover on it"
    );
    assert!(
        deck_still.on_bridge,
        "placing a mover under the deck cleared the deck mover's on_bridge flag"
    );
    assert_eq!(
        i16::from(under_z as i8),
        i16::from(cut.deck_terrain_level as i8),
        "the under mover is not at the cell's own ground level"
    );
    assert!(!under_on_bridge, "the under mover carries on_bridge");
    assert_eq!(under_occ, None, "the under mover holds a BridgeOccupancy");
    assert_ne!(
        under_z, deck_still.position.z,
        "the two movers share one height on one cell"
    );

    // 3. Drive the under mover out with an ordinary undisabled order and hold
    //    every stamped frame to the under-span invariant.
    assert!(
        issue_ordinary_move(&mut scenario, &owner_name, under_id, exit),
        "the ordinary Move out from under the span was refused"
    );
    let rows = record_until(&mut scenario, under_id, exit);
    print_tick_table(&rows);
    let under_frames: Vec<&TickRow> = rows.iter().filter(|row| row.structural).collect();
    println!(
        "{} frame(s) leaving the underside, {} of them on a stamped cell; last {:?}",
        rows.len(),
        under_frames.len(),
        rows.last().map(|row| row.cell),
    );
    assert!(
        !under_frames.is_empty(),
        "the under mover never occupied a stamped cell, so nothing about the under-span state \
         was observed"
    );
    assert_under_span_invariant(&under_frames);
    let violations: Vec<&TickRow> = rows.iter().filter(|row| !row.holds_invariant()).collect();
    assert!(
        violations.is_empty(),
        "position.z left the native model on {} frame(s); first: {:?}",
        violations.len(),
        violations.first(),
    );

    // The deck mover is untouched by all of it.
    let deck_end = scenario
        .sim()
        .entities()
        .get(deck_id)
        .expect("deck mover survived the under mover's drive");
    println!(
        "deck mover after the under drive: cell ({},{}) z={} on_bridge={}",
        deck_end.position.rx, deck_end.position.ry, deck_end.position.z, deck_end.on_bridge
    );
    assert_eq!(
        (deck_end.position.rx, deck_end.position.ry),
        shared,
        "the deck mover moved while a unit drove out from underneath it"
    );
    assert!(
        deck_end.on_bridge,
        "the deck mover lost on_bridge while a unit drove out from underneath it"
    );
}

/// Inventory only: which stock maps expose a high-bridge span at all, and what
/// the deck/approach levels are. Diagnostic — it asserts nothing about movement.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_high_bridge_inventory() {
    let Some(retail) = retail_dir() else {
        eprintln!("SKIPPED: no retail root (set RA2_DIR or provide config.toml)");
        return;
    };
    for map_file in [
        "BayOPigs.mmx",
        "Dustbowl.mmx",
        "Bermuda.mmx",
        "Lostlake.mmx",
        "Hills.mmx",
    ] {
        match headless_scenario::load(&retail, map_file, SEED) {
            Ok(scenario) => {
                let grid = scenario.sim().path_grid().expect("navigation published");
                let structural = (0..grid.height())
                    .flat_map(|y| (0..grid.width()).map(move |x| (x, y)))
                    .filter(|(x, y)| grid.cell(*x, *y).is_some_and(|c| c.bridge_structural))
                    .count();
                match find_high_bridge_span(grid) {
                    Some(span) => println!(
                        "{map_file}: {structural} structural cell(s); best span {} cell(s) at \
                         terrain level {} between {:?} and {:?} (approach level {})",
                        span.deck.len(),
                        span.deck_terrain_level,
                        span.approach_a,
                        span.approach_b,
                        span.approach_level,
                    ),
                    None => println!("{map_file}: {structural} structural cell(s); no usable span"),
                }
            }
            Err(error) => println!("{map_file}: load failed: {error}"),
        }
    }
}

/// Scale instrument, not a parity test: N tanks on `Hills.mmx` all ordered
/// across the valley, per-frame wall time of the whole simulation frame.
/// `VERA20K_SCALE_MOVERS` overrides the count (default 400).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn scale_benchmark_many_movers_on_hills() {
    let Some(retail) = retail_dir() else {
        return;
    };
    let movers: usize = std::env::var("VERA20K_SCALE_MOVERS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(400);
    let frames: usize = std::env::var("VERA20K_SCALE_FRAMES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(150);
    let mut scenario = headless_scenario::load(&retail, "Hills.mmx", SEED).expect("Hills loads");
    let owner_name = prepare_commanding_house(&mut scenario);
    let mut ids = Vec::new();
    'spawn: for ry in 40..120u16 {
        for rx in 40..120u16 {
            if ids.len() >= movers {
                break 'spawn;
            }
            let SimRuntime {
                simulation,
                resources,
            } = &mut scenario.runtime;
            if let Some(id) = simulation.spawn_object(
                "MTNK",
                &owner_name,
                rx,
                ry,
                0,
                &resources.rules,
                &resources.height_map,
            ) {
                ids.push((id, rx, ry));
            }
        }
    }
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        simulation.resolve_type_handles(&resources.rules);
    }
    let owner_id = scenario
        .sim()
        .interner
        .get(&owner_name)
        .expect("owner interned");
    let execute_tick = scenario.sim().session.tick + 1;
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&(id, rx, ry)| {
            CommandEnvelope::new(
                owner_id,
                execute_tick,
                Command::Move {
                    entity_id: id,
                    target_rx: rx.wrapping_add(30),
                    target_ry: ry,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();
    let live_objects = scenario.sim().entities().len();
    let mut total = std::time::Duration::ZERO;
    let mut worst = std::time::Duration::ZERO;
    for frame in 0..frames {
        let batch = if frame == 0 { commands.as_slice() } else { &[] };
        let started = std::time::Instant::now();
        scenario
            .runtime
            .advance_frame(batch, SIM_TICK_MS, TickLane::Ordinary)
            .expect("frame completes");
        let elapsed = started.elapsed();
        total += elapsed;
        worst = worst.max(elapsed);
    }
    let moved = ids
        .iter()
        .filter(|&&(id, rx, ry)| {
            scenario
                .sim()
                .entities()
                .get(id)
                .is_some_and(|entity| (entity.position.rx, entity.position.ry) != (rx, ry))
        })
        .count();
    println!(
        "SCALE movers={} live_objects={live_objects} frames={frames} avg_ms={:.2} worst_ms={:.2} moved={moved}",
        ids.len(),
        total.as_secs_f64() * 1000.0 / frames as f64,
        worst.as_secs_f64() * 1000.0,
    );
}
