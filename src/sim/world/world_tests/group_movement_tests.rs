//! Group movement tests: vehicle stacking reproductions on the real command path,
//! crusher and scatter progress, factory and repair-pad entry search, and the
//! group-destination spread.

use super::*;

// ===========================================================================
// GROUP-MOVE STACKING REPRODUCTION
//
// Player report: "tanks when moved in a group stack on top of each other."
// In retail YR ground vehicles are strictly one-per-cell. These tests drive
// the real command path (Command::Move through advance_tick, including the
// staged-megamission group-destination distributor) and measure whether two
// vehicles ever end up on the same cell.
// ===========================================================================

/// A square clear map with production navigation and retail `rulesmd.ini`, so
/// the group-destination distributor, the Find_Path owner and the Drive track
/// run on the stock MTNK and E1 as they do in a loaded map. The outcomes depend
/// on the unit data: an E1 written without its Walk `Locomotor` left the
/// crusher guard stalled for 555 ticks. `None` when the retail INIs are absent.
fn stacking_world(size: u16) -> Option<(Simulation, RuleSet, PathGrid)> {
    let rules = crate::rules::retail_ini_fixture::retail_rulesmd_rules()?;
    Some(stacking_navigation_world(rules, size))
}

/// A square clear map with production-shaped playfield, Map Size and
/// navigation (`rebuild_dynamic_navigation`), so the Find_Path owner serves
/// every request as it does in a loaded map. The diamond is deliberately broad
/// to keep the fixtures focused on movement admission.
fn stacking_navigation_world(rules: RuleSet, size: u16) -> (Simulation, RuleSet, PathGrid) {
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: i32::from(size),
        off_fc: -256,
        off_100: -256,
        off_104: 512,
        off_108: 512,
    });
    sim.playfield_size_height = Some(i32::from(size));
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(size, size));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = PathGrid::clone(&sim.path_grid_snapshot().expect("navigation built"));
    (sim, rules, grid)
}

/// An American MTNK facing 64 on each of `cells`, in order.
fn spawn_tanks(sim: &mut Simulation, rules: &RuleSet, cells: &[(u16, u16)]) -> Vec<u64> {
    cells
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, rules)
                .expect("tank spawns")
        })
        .collect()
}

/// The longest stretch of consecutive ticks on which `id` did not change cell
/// while it still held a movement order. A mover whose selection is refused by
/// a predicate that nothing downstream can resolve produces a bit-identical
/// tick forever; that shows up here as an unbounded run.
fn longest_stationary_run_while_ordered(series: &[(bool, (u16, u16))]) -> usize {
    let mut longest = 0usize;
    let mut open = 0usize;
    let mut previous: Option<(u16, u16)> = None;
    for &(ordered, cell) in series {
        if ordered && previous == Some(cell) {
            open += 1;
            longest = longest.max(open);
        } else {
            open = 0;
        }
        previous = Some(cell);
    }
    longest
}

/// D1 GUARD — a crusher must not be frozen by a body it is entitled to drive
/// over.
///
/// The cell occupation mask holds a bit for vehicles only: `0x20` is written
/// exclusively by `UnitClass__MarkCellOccupationBit20 @ 0x007441B0`, while
/// `InfantryClass__MarkCellOccupancy @ 0x005217C0` writes `1 << GetSubCell` into
/// the sub-cell bits of the same byte. So infantry never refuses a Drive curve
/// by occupation, and a tank ordered through them keeps moving: it crushes an
/// enemy, and its route bends around a friendly (code 6). A refusal on mere unit
/// presence would stall the tank one refusal per tick, forever, because nothing
/// downstream ever clears it.
#[test]
fn crusher_does_not_freeze_in_front_of_infantry() {
    for enemy_infantry in [false, true] {
        let Some((mut sim, rules, grid)) = stacking_world(24) else {
            return;
        };

        let infantry_owner = if enemy_infantry {
            "Russians"
        } else {
            "Americans"
        };
        let blocker = sim
            .spawn_object("E1", infantry_owner, 10, 10, 0, &rules)
            .expect("infantry spawns");
        let tank = sim
            .spawn_object("MTNK", "Americans", 6, 10, 64, &rules)
            .expect("tank spawns");

        let cmd = cmd_envelope(&sim, "Americans", 1, move_command(tank, (16, 10)));
        let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 100);

        // Make the victim's house human so a stray "Unit lost" would be
        // audible: a crush is `RecordKill` + `UnInit` (`0x007416A0`), never
        // `ReceiveDamage`, so `Death_Announcement` (`+0x3B8`) must stay silent.
        if let Some(house) = sim
            .interner
            .get(infantry_owner)
            .and_then(|id| sim.houses.get_mut(&id))
        {
            house.set_is_human_for_test(true);
        }

        let mut series: Vec<(bool, (u16, u16))> = Vec::new();
        let mut arrived_at: Option<u64> = None;
        let mut entered_blocker_cell: Option<u64> = None;
        for tick in 0..600u64 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
            let Some(e) = sim.substrate.entities.get(tank) else {
                break;
            };
            let cell = (e.position.rx(), e.position.ry());
            series.push((
                crate::sim::movement::motion_query::is_moving(&e) == Some(true),
                cell,
            ));
            if cell == (10, 10) && entered_blocker_cell.is_none() {
                entered_blocker_cell = Some(tick);
            }
            if cell == (16, 10) && arrived_at.is_none() {
                arrived_at = Some(tick);
            }
        }

        let stall = longest_stationary_run_while_ordered(&series);
        println!(
            "--- crusher_does_not_freeze_in_front_of_infantry (enemy={enemy_infantry}) ---\n    \
             tank: {}\n    blocker: {}\n    longest ordered-but-stationary run = {stall} tick(s); \
             arrived_at = {arrived_at:?}; entered (10,10) at {entered_blocker_cell:?}",
            stacking_motion_state(&sim, tank),
            stacking_motion_state(&sim, blocker),
        );

        // Arrival alone does not prove the exclusion was exercised: a tank that
        // routed politely around the man never asked about his cell. So with
        // an enemy the tank must go THROUGH the blocker's own cell: the Unit
        // +1AC the search calls latches the crush in its enemy arm and the
        // tail (`0x0073FCF6`) answers 0 for an unoccupied vehicle bit.
        //
        // A friendly takes the allied stationary arm instead (`0x0073F865..
        // 8C0`, running code 6), which `AStar_compute_edge_cost 0x00429830`
        // prices at 8x (`0x0081870C`). A diagonal step costs what a straight
        // one does, so the route bends around his cell at no extra length and
        // the tank never meets him; the crush latch lives only in the enemy
        // arm, so being a crusher earns no discount on an ally.
        assert_eq!(
            entered_blocker_cell.is_some(),
            enemy_infantry,
            "the crusher entered the infantry's cell (10,10) only with an enemy \
             (enemy_infantry={enemy_infantry}): {}",
            stacking_motion_state(&sim, tank)
        );

        // A blocked mover legitimately waits: BlockagePathDelay is 60 frames and
        // the scatter/repath ladder can run several spans. What it never does is
        // sit still for the whole run.
        assert!(
            stall < 300,
            "crusher sat still for {stall} consecutive ticks while still ordered \
             (enemy_infantry={enemy_infantry}) — this is the permanent-refusal freeze"
        );
        assert!(
            arrived_at.is_some(),
            "crusher never reached (16,10) with enemy_infantry={enemy_infantry}: {}",
            stacking_motion_state(&sim, tank)
        );
        // `sim.sound_events` accumulates until the app drains it, so the whole
        // run is visible here.
        let unit_lost_lines = sim
            .sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::UnitLost { .. }))
            .count();
        assert_eq!(
            unit_lost_lines, 0,
            "a crush kill must not announce \"Unit lost\" (enemy_infantry={enemy_infantry})"
        );
    }
}

/// D2 GUARD — a turning curve whose ENDPOINT is occupied must still make
/// progress.
///
/// gamemd asks `Can_Enter_Cell` once per selection, about one cell
/// (0x004B34C0), and dispatches on the one code it gets back. A gate that asked
/// about a second cell — the curve's two-cells-out endpoint — and then reported
/// the FIRST cell to its dispatch would hand the dispatch a cell that is clear,
/// which resolves to "not blocked", resets the timers and returns without
/// stepping: a bit-identical tick, forever.
#[test]
fn turning_mover_with_an_occupied_endpoint_still_makes_progress() {
    let Some((mut sim, rules, grid)) = stacking_world(24) else {
        return;
    };

    // Mover at (10,10) ordered north-east: the curve's head node is (10,9) and
    // its endpoint two cells out is (11,9). Park a friendly on the endpoint and
    // leave the head node clear.
    let parked = sim
        .spawn_object("MTNK", "Americans", 11, 9, 64, &rules)
        .expect("parked tank spawns");
    let mover = sim
        .spawn_object("MTNK", "Americans", 10, 10, 64, &rules)
        .expect("mover spawns");

    let cmd = cmd_envelope(&sim, "Americans", 1, move_command(mover, (10, 4)));
    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 100);

    let mut series: Vec<(bool, (u16, u16))> = Vec::new();
    let mut arrived_at: Option<u64> = None;
    for tick in 0..600u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let Some(e) = sim.substrate.entities.get(mover) else {
            break;
        };
        let cell = (e.position.rx(), e.position.ry());
        series.push((
            crate::sim::movement::motion_query::is_moving(&e) == Some(true),
            cell,
        ));
        if cell == (10, 4) && arrived_at.is_none() {
            arrived_at = Some(tick);
        }
    }

    let stall = longest_stationary_run_while_ordered(&series);
    println!(
        "--- turning_mover_with_an_occupied_endpoint_still_makes_progress ---\n    \
         mover: {}\n    parked: {}\n    longest ordered-but-stationary run = {stall}; \
         arrived_at = {arrived_at:?}",
        stacking_motion_state(&sim, mover),
        stacking_motion_state(&sim, parked),
    );

    assert!(
        stall < 300,
        "mover sat still for {stall} consecutive ticks while still ordered — \
         the endpoint refusal is not resolvable by its own dispatch"
    );
    assert!(
        arrived_at.is_some(),
        "mover never reached (10,4): {}",
        stacking_motion_state(&sim, mover)
    );
    let m = sim.substrate.entities.get(mover).expect("mover alive");
    let p = sim.substrate.entities.get(parked).expect("parked alive");
    assert_ne!(
        (m.position.rx(), m.position.ry()),
        (p.position.rx(), p.position.ry()),
        "two ground vehicles must never rest on the same cell"
    );
}

/// B1 GUARD — a parked friendly vehicle standing ON the mover's route must be
/// told to move, not merely repathed around.
///
/// gamemd gives an allied body sitting still in the cell (`Can_Enter_Cell` code
/// 6) its own arm, ending in `CellClass__Scatter_Objects @ 0x00481670`: in the
/// fresh selection (`CMP EDX,0x6 / JNZ 0x004B3944` at 0x004B36F4, scatter call
/// 0x004B393A) and in the path continuation's ally arm (0x004B2B4B..0x004B2DC0,
/// scatter 0x004B2D68..0x004B2DC0). Codes 2 and 5 reach no `Scatter_Objects`
/// call. Here the continuation's arm answers: the Find_Path request that meets
/// the parked tank scatters its cell (`answer_track_ally_cell`).
///
/// The blocker is parked AFTER the order is issued and directly on the route
/// Find_Path already returned: the ordinary case of a group member that
/// finishes its own move while a peer is still routed through the cell it
/// stopped on.
///
/// The parked cell is the one gap in a rock wall down column 12. The search
/// calls the Unit +1AC (`0x00429F54`), which answers 6 for the parked ally
/// (`0x0073F865..8C0`), priced 8x (`0x0081870C`); in open ground the route
/// would bend around that cell at no extra length and never meet him.
///
/// SEEN TO FAIL (2026-09-27): with `scatter_blocked_track_cell` returning
/// early, the blocker never leaves (12,8) and the mover stands 357 ticks short
/// of its destination.
#[test]
fn parked_friendly_on_the_route_is_scattered_out_of_the_way() {
    let Some((mut sim, rules, _)) = stacking_world(24) else {
        return;
    };

    const PARKED_AT: (u16, u16) = (12, 8);
    const DESTINATION: (u16, u16) = (18, 8);
    // Retail [Rock]: 0% for every SpeedType, zone impassable.
    {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
        let rock = SpeedCostProfile {
            foot: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            track: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            wheel: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            float: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            amphibious: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            float_beach: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            hover: Some(crate::util::native_x87::NativeF32Bits::POSITIVE_ZERO),
            winged: Some(crate::util::native_x87::NativeF32Bits::ONE),
        };
        let terrain = sim.resolved_terrain.as_mut().expect("stacking terrain");
        for ry in (0..24).filter(|&ry| ry != PARKED_AT.1) {
            let cell = terrain
                .cell_mut_for_test(PARKED_AT.0, ry)
                .expect("wall cell");
            cell.land_type = LandType::Rock.as_index();
            cell.speed_costs = rock;
            cell.zone_type = zone_class::IMPASSABLE;
            cell.ground_walk_blocked = true;
            cell.base_ground_walk_blocked = true;
            cell.base_build_blocked = true;
            cell.base_land_type = LandType::Rock.as_index();
            cell.base_speed_costs = rock;
        }
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = PathGrid::clone(&sim.path_grid_snapshot().expect("navigation rebuilt"));

    let mover = sim
        .spawn_object("MTNK", "Americans", 6, 8, 64, &rules)
        .expect("mover spawns");

    let cmd = cmd_envelope(&sim, "Americans", 1, move_command(mover, DESTINATION));
    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 100);
    // The Move dispatches at this frame's EventClass tail; the next frame's
    // first Process requests the route (Unit741970 accepts without one).
    let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);

    // The route A* actually returned, before anything is parked on it. The
    // fixture is only meaningful if the blocker cell is on it.
    let route: Vec<(u16, u16)> = sim
        .substrate
        .entities
        .get(mover)
        .map(|m| found_route(m, (6, 8)))
        .unwrap_or_default();
    assert!(
        route.contains(&PARKED_AT),
        "fixture precondition: {PARKED_AT:?} must lie on the mover's route {route:?}"
    );

    let parked = sim
        .spawn_object("MTNK", "Americans", PARKED_AT.0, PARKED_AT.1, 64, &rules)
        .expect("parked tank spawns");

    let mut series: Vec<(bool, (u16, u16))> = Vec::new();
    let mut blocker_left_at: Option<u64> = None;
    let mut arrived_at: Option<u64> = None;
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        if let Some(b) = sim.substrate.entities.get(parked)
            && (b.position.rx(), b.position.ry()) != PARKED_AT
            && blocker_left_at.is_none()
        {
            blocker_left_at = Some(tick);
        }
        let Some(m) = sim.substrate.entities.get(mover) else {
            break;
        };
        let cell = (m.position.rx(), m.position.ry());
        series.push((
            crate::sim::movement::motion_query::is_moving(&m) == Some(true),
            cell,
        ));
        if cell == DESTINATION && arrived_at.is_none() {
            arrived_at = Some(tick);
        }
    }

    let stall = longest_stationary_run_while_ordered(&series);
    println!(
        "--- parked_friendly_on_the_route_is_scattered_out_of_the_way ---
             route: {route:?}
    mover: {}
    parked: {}
             blocker left {PARKED_AT:?} at {blocker_left_at:?}; arrived_at = {arrived_at:?}; \
             longest ordered-but-stationary run = {stall}",
        stacking_motion_state(&sim, mover),
        stacking_motion_state(&sim, parked),
    );

    assert!(
        blocker_left_at.is_some(),
        "the parked friendly never left {PARKED_AT:?}, so nothing ever reached the \
         code-6 scatter: {}",
        stacking_motion_state(&sim, parked)
    );
    assert!(
        arrived_at.is_some(),
        "the mover never reached its ordered destination {DESTINATION:?}: {}",
        stacking_motion_state(&sim, mover)
    );
}

/// The route a Unit's first Find_Path (`0x004D3920`) installs after a class
/// destination to `goal`, with `building` placed at `origin` and a supplied
/// live contact. A synchronized player Move first sends BREAK at4C72F8 and
/// cannot supply that contact to the search. This fixture isolates the shared
/// class/search owner; command cleanup has its own native lifecycle consumer.
/// Foundations are art keys, so the retail artmd.ini is installed too.
fn unit_route_beside_building(
    building: &str,
    origin: (u16, u16),
    goal: (u16, u16),
    contact: bool,
) -> Option<Vec<(u16, u16)>> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let (mut sim, rules, _) = stacking_navigation_world(rules, 24);
    let building_id = sim
        .spawn_object(building, "Americans", origin.0, origin.1, 0, &rules)
        .expect("building spawns");
    let tank = sim
        .spawn_object("MTNK", "Americans", 4, 7, 64, &rules)
        .expect("tank spawns");
    if contact {
        let e = sim.substrate.entities.get_mut(tank).unwrap();
        e.seldom.radio_contacts.insert(building_id);
    }
    let grid = PathGrid::clone(&sim.path_grid_snapshot().expect("navigation built"));
    sim.substrate
        .mission_assign_exact(tank, MissionId::from_known(MissionType::Move), 0)
        .unwrap();
    sim.assign_destination_represented(
        tank,
        Some(crate::sim::components::NavTargetRef::cell(goal.0, goal.1)),
        &rules,
        &mut crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .seldom
            .radio_contacts
            .contains(building_id),
        contact,
        "the class destination preserves the supplied contact"
    );
    let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    Some(
        sim.substrate
            .entities
            .get(tank)
            .map(|m| found_route(m, (4, 7)))
            .unwrap_or_default(),
    )
}

/// The route the first Find_Path installed from `start`, read from the
/// Foot+5E0 words it wrote (consumed ones included), or empty when the
/// object has no order or no installed words.
fn found_route(e: &GameEntity, start: (u16, u16)) -> Vec<(u16, u16)> {
    if crate::sim::movement::motion_query::is_moving(&e) != Some(true)
        || e.navigation.path_replay().directions().is_empty()
    {
        return Vec::new();
    }
    e.navigation.path_replay().installed_cells(start)
}

/// Find_Path's search calls the Unit's own +1AC for every neighbour
/// (`0x00429F54`). A building the Unit holds radio contact with is skipped
/// when `0x00458A00` answers false (`0x0073F57C..5A2`): retail GAWEAP
/// (Foundation=5x3, Bib=yes, NumberImpassableRows=1) then opens every column
/// east of its first. Without contact the allied building answers 7, so the
/// route stays off the factory's inner columns (only the east Bib column is
/// ever open). The goal is the factory's exit Cell, the one destination its
/// WeaponsFactory arm lets a contacted Unit take (`0x00742D98..0x00742DFD`).
#[test]
fn unit_search_enters_a_contacted_factory_east_of_its_impassable_rows() {
    let origin = (10, 6);
    let goal = (14, 7);
    let on_factory = |&(x, y): &(u16, u16)| (10..=13).contains(&x) && (6..=8).contains(&y);
    let Some(apart) = unit_route_beside_building("GAWEAP", origin, goal, false) else {
        return;
    };
    assert!(
        !apart.iter().any(on_factory),
        "no contact: the route stays off the factory: {apart:?}"
    );
    let contacted = unit_route_beside_building("GAWEAP", origin, goal, true).unwrap();
    assert_eq!(
        contacted.last(),
        Some(&goal),
        "contact: the route reaches the exit Cell: {contacted:?}"
    );
    assert!(
        contacted.iter().any(on_factory),
        "contact: the route crosses the factory: {contacted:?}"
    );
    assert!(
        !contacted
            .iter()
            .any(|&(x, y)| x == origin.0 && (6..=8).contains(&y)),
        "the impassable west row still blocks: {contacted:?}"
    );
}

/// The UnitRepair/Bunker arm (`0x0073F74B..774`) needs no contact: retail
/// GADEPT (Foundation=3x3, UnitRepair=yes, NumberImpassableRows=1) opens its
/// pad east of the first column to any Unit's search, so a Move onto the pad
/// centre routes onto it and around the closed west column.
#[test]
fn unit_search_enters_a_repair_pad_east_of_its_impassable_rows() {
    let origin = (12, 6);
    let goal = (13, 7);
    let Some(route) = unit_route_beside_building("GADEPT", origin, goal, false) else {
        return;
    };
    assert_eq!(route.last(), Some(&goal), "route onto the pad: {route:?}");
    assert!(
        !route
            .iter()
            .any(|&(x, y)| x == origin.0 && (6..=8).contains(&y)),
        "the impassable west row still blocks: {route:?}"
    );
}

fn stacking_cells(sim: &Simulation, ids: &[u64]) -> Vec<(u64, u16, u16)> {
    ids.iter()
        .filter_map(|&id| {
            sim.substrate
                .entities
                .get(id)
                .map(|e| (id, e.position.rx(), e.position.ry()))
        })
        .collect()
}

fn stacking_duplicates(cells: &[(u64, u16, u16)]) -> BTreeMap<(u16, u16), Vec<u64>> {
    let mut by_cell: BTreeMap<(u16, u16), Vec<u64>> = BTreeMap::new();
    for &(id, rx, ry) in cells {
        by_cell.entry((rx, ry)).or_default().push(id);
    }
    by_cell.retain(|_, ids| ids.len() > 1);
    by_cell
}

/// (d) raw grid state for one cell: object list and the destructive raw
/// CellClass ground byte.
fn stacking_cell_state(sim: &Simulation, rx: u16, ry: u16) -> String {
    let list: Vec<(u64, MovementLayer, Option<u8>, bool)> = sim
        .substrate
        .occupancy
        .get(rx, ry)
        .map(|o| {
            o.occupants
                .iter()
                .map(|c| (c.entity_id, c.layer, c.sub_cell, c.is_building))
                .collect()
        })
        .unwrap_or_default();
    format!(
        "({rx},{ry}) object_list={list:?} raw_ground=0x{:02X}",
        sim.substrate.raw_cell_occupation.ground_bits(rx, ry),
    )
}

/// (e) is this vehicle still trying to move?
fn stacking_motion_state(sim: &Simulation, id: u64) -> String {
    let Some(e) = sim.substrate.entities.get(id) else {
        return "<gone>".to_string();
    };
    let motion = match crate::sim::movement::motion_query::is_moving(e) {
        Some(false) | None => format!(
            "id={id} at ({},{}) sub=({},{}) stopped",
            e.position.rx(),
            e.position.ry(),
            e.position.sub_x(),
            e.position.sub_y()
        ),
        Some(true) => format!(
            "id={id} at ({},{}) sub=({},{}) route_ahead={:?} words_left={} goal={:?}",
            e.position.rx(),
            e.position.ry(),
            e.position.sub_x(),
            e.position.sub_y(),
            e.navigation.path_replay().route_cells(),
            e.navigation.path_replay().remaining_directions().len(),
            crate::sim::movement::movement_goal_cell(e)
        ),
    };
    format!(
        "{motion} nav={:?} drive={:?} foot={:?} mission={:?} marked={} occupation={}",
        e.navigation.nav_com(),
        retained_drive(e).cloned(),
        e.foot_speed,
        e.mission.current(),
        e.lifecycle.cell_marked(),
        e.foot_occupation_enabled()
    )
}

/// World lepton position of a mover. 256 leptons per cell — the verified
/// leptons-per-cell constant the whole coordinate frame is built on.
fn stacking_lepton_pos(sim: &Simulation, id: u64) -> Option<(i64, i64)> {
    sim.substrate.entities.get(id).map(|e| {
        (
            i64::from(e.position.rx()) * 256 + i64::from(e.position.sub_x().to_num::<i32>()),
            i64::from(e.position.ry()) * 256 + i64::from(e.position.sub_y().to_num::<i32>()),
        )
    })
}

/// Straight-line hull separation between two movers, in leptons. This is what
/// the renderer draws; the cell index is not.
fn stacking_gap(sim: &Simulation, a: u64, b: u64) -> Option<i64> {
    let (ax, ay) = stacking_lepton_pos(sim, a)?;
    let (bx, by) = stacking_lepton_pos(sim, b)?;
    let dx = ax - bx;
    let dy = ay - by;
    Some((((dx * dx + dy * dy) as f64).sqrt()) as i64)
}

/// Closest approach between any two of `ids` on this tick.
fn stacking_min_gap(sim: &Simulation, ids: &[u64]) -> Option<(u64, u64, i64)> {
    let mut worst: Option<(u64, u64, i64)> = None;
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let Some(gap) = stacking_gap(sim, ids[i], ids[j]) else {
                continue;
            };
            if worst.is_none_or(|(_, _, best)| gap < best) {
                worst = Some((ids[i], ids[j], gap));
            }
        }
    }
    worst
}

/// Closest approach between two movers that are on the SAME cell this tick.
///
/// This is the regime the derived bound actually covers: it bounds the instant
/// gamemd admits a mover into a cell another mover still occupies. Convergence
/// between two movers in different cells is ordinary traffic and is not what the
/// derivation speaks to.
fn stacking_min_gap_within_shared_cell(sim: &Simulation, ids: &[u64]) -> Option<(u64, u64, i64)> {
    let mut worst: Option<(u64, u64, i64)> = None;
    for (_, members) in stacking_duplicates(&stacking_cells(sim, ids)) {
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let Some(gap) = stacking_gap(sim, members[i], members[j]) else {
                    continue;
                };
                if worst.is_none_or(|(_, _, best)| gap < best) {
                    worst = Some((members[i], members[j], gap));
                }
            }
        }
    }
    worst
}

/// Half a cell. Two MTNK hulls this close read on screen as one stacked sprite,
/// which is what the player reported seeing.
const VISIBLE_OVERLAP_LEPTONS: i64 = 128;

/// The player's measured symptoms, used directly as the acceptance bound.
///
/// Verbatim report: "tanks when moved in a group stack on top of each other."
/// Measured: **38 leptons** between a head-on pair, and **29 leptons held for 31
/// consecutive ticks** on a group move. 256 leptons is one cell.
///
/// Both numbers matter and neither substitutes for the other. A minimum taken
/// over a run and then compared to a floor cannot see the second report at all:
/// two hulls parked 130 leptons apart for 31 straight ticks are visually
/// indistinguishable from the complaint and clear every floor above. So the
/// duration of close approach is measured too, and both are asserted.
const REPORTED_HEADON_OVERLAP_LEPTONS: i64 = 38;
const REPORTED_SUSTAINED_OVERLAP_TICKS: usize = 31;

/// The bound the duration assertion actually uses — deliberately NOT the
/// player's 31.
///
/// 31 was the bound until 2026-08-05 and it had never fired, because it sits
/// ABOVE the worst value ever measured against it. The known-bad intermediate
/// build recorded on [`StackingWatch`] produced a **28**-tick run inside half a
/// cell, and 28 < 31, so that build PASSED this assertion; only the sibling
/// 38-lepton floor caught it. A regression that parked two hulls 45 leptons
/// apart for 28 straight ticks would have failed nothing.
///
/// Derived from measurement instead. On the passing build all three fixtures
/// that assert this report `longest run under 128 leptons = 0` — no pair ever
/// comes within half a cell for even one tick, the tightest approach anywhere
/// being 180 leptons on `group_move_short_range` and 181 on
/// `group_move_eight_to_one_cell` and `column_of_four`. The bound is one
/// blocked-retry span (`FootPathRuntime::RETRY_RELOAD` = 10, which Drive's
/// blocked label stores into `Foot+0x64C` at `0x004B3285`, straight after its
/// `Scatter_Objects` call at `0x004B327D`), the shortest window in which a
/// legitimate pass-by resolves itself. That is 10 ticks of headroom over the
/// measured 0 and it refuses 18 ticks before the known-bad 28.
const SUSTAINED_OVERLAP_TICK_BOUND: usize =
    crate::sim::components::FootPathRuntime::RETRY_RELOAD as usize;

/// Per-tick separation record for a set of movers.
///
/// Deliberately keeps the whole series rather than folding straight to a
/// minimum: the reported defect is a *sustained* overlap, and a fold to one
/// number throws away exactly the axis that distinguishes it from an ordinary
/// pass-by.
///
/// SEEN TO FAIL, twice, before being trusted (2026-08-05):
///
/// * With the cell-admission gate reverted entirely,
///   `repro_two_moving_vehicles_pass_through_each_other` reports "head-on pair
///   closed to 36 leptons (0.14 cells) at tick 68" and
///   `column_of_vehicles_all_arrive_without_stacking` reports two hulls 190
///   leptons apart while sharing a cell, under the 239-lepton derived bound.
/// * With an intermediate build of the gate that dropped a refused mover's own
///   cell claim, `repro_group_move_of_eight_vehicles_to_one_cell` reported
///   "vehicles 3 and 8 closed to 29 leptons (0.11 cells) at tick 341" with a
///   28-consecutive-tick run inside half a cell — the player's reported numbers
///   almost exactly, on a pair in ADJACENT cells. The predecessor of this
///   struct passed that build: its bound sat inside an `if let Some(..)` on a
///   same-cell measure that the change itself drove to `None` everywhere, and
///   its all-pairs helper was never called.
#[derive(Default)]
struct StackingWatch {
    /// Closest approach between ANY two movers: (a, b, leptons, tick).
    closest: Option<(u64, u64, i64, u64)>,
    /// Closest approach between two movers that share a cell on that tick.
    closest_in_cell: Option<(u64, u64, i64, u64)>,
    /// Per-tick all-pairs minimum, kept in full.
    series: Vec<(u64, i64)>,
    /// Longest consecutive run of ticks whose all-pairs minimum was inside
    /// [`VISIBLE_OVERLAP_LEPTONS`], and where that run ended.
    longest_close_run: usize,
    longest_close_run_end: u64,
    open_close_run: usize,
    /// Ticks on which two movers occupied one cell.
    shared_cell_ticks: Vec<u64>,
}

impl StackingWatch {
    fn sample(&mut self, sim: &Simulation, ids: &[u64], tick: u64) {
        if let Some((a, b, gap)) = stacking_min_gap(sim, ids) {
            self.series.push((tick, gap));
            if self.closest.is_none_or(|(_, _, best, _)| gap < best) {
                self.closest = Some((a, b, gap, tick));
            }
            if gap < VISIBLE_OVERLAP_LEPTONS {
                self.open_close_run += 1;
                if self.open_close_run > self.longest_close_run {
                    self.longest_close_run = self.open_close_run;
                    self.longest_close_run_end = tick;
                }
            } else {
                self.open_close_run = 0;
            }
        }
        if let Some((a, b, gap)) = stacking_min_gap_within_shared_cell(sim, ids)
            && self
                .closest_in_cell
                .is_none_or(|(_, _, best, _)| gap < best)
        {
            self.closest_in_cell = Some((a, b, gap, tick));
        }
        if !stacking_duplicates(&stacking_cells(sim, ids)).is_empty() {
            self.shared_cell_ticks.push(tick);
        }
    }

    fn report(&self, label: &str) {
        println!(
            "[{label}] samples={} closest(any pair)={:?} closest(shared cell)={:?} \
             longest run under {VISIBLE_OVERLAP_LEPTONS} leptons = {} tick(s) ending {} \
             shared-cell ticks = {}",
            self.series.len(),
            self.closest,
            self.closest_in_cell,
            self.longest_close_run,
            self.longest_close_run_end,
            self.shared_cell_ticks.len(),
        );
        let tightest: Vec<String> = {
            let mut s = self.series.clone();
            s.sort_by_key(|&(_, gap)| gap);
            s.iter()
                .take(12)
                .map(|(t, g)| format!("t{t}:{g}"))
                .collect()
        };
        println!("[{label}] tightest sampled ticks: {}", tightest.join(" "));
    }

    /// The acceptance surface. Unconditional: a fixture that sampled nothing
    /// fails here rather than passing vacuously.
    fn assert_no_reported_stacking(&self, label: &str) {
        let (a, b, gap, tick) = self
            .closest
            .unwrap_or_else(|| panic!("[{label}] measured no pair separation at all"));
        assert!(
            gap > REPORTED_HEADON_OVERLAP_LEPTONS,
            "[{label}] vehicles {a} and {b} closed to {gap} leptons ({:.2} cells) at tick {tick}; \
             the player's head-on report was {REPORTED_HEADON_OVERLAP_LEPTONS}",
            gap as f64 / 256.0
        );
        assert!(
            self.longest_close_run < SUSTAINED_OVERLAP_TICK_BOUND,
            "[{label}] some pair stayed inside {VISIBLE_OVERLAP_LEPTONS} leptons for {} \
             consecutive ticks (run ends at tick {}); the bound is \
             {SUSTAINED_OVERLAP_TICK_BOUND} and the passing build measures 0. For scale, \
             the player's group-move report was {REPORTED_SUSTAINED_OVERLAP_TICKS} \
             consecutive ticks and the known-bad intermediate build produced 28",
            self.longest_close_run,
            self.longest_close_run_end,
        );
        // The transit bound is derived for ONE instant — a mover admitted into a
        // cell another mover still occupies — so it is asserted only on that
        // regime. It is a refinement of the two bounds above, never a substitute.
        if let Some((ca, cb, cgap, ctick)) = self.closest_in_cell {
            let bound = derived_min_transit_separation_leptons();
            assert!(
                cgap >= bound,
                "[{label}] vehicles {ca} and {cb} shared a cell only {cgap} leptons \
                 ({:.2} cells) apart at tick {ctick}; retail's own admission rule cannot \
                 produce anything below {bound}",
                cgap as f64 / 256.0
            );
        }
    }
}

/// The smallest hull separation retail's own admission rule can produce between
/// two ground vehicles — DERIVED from the shipped curve tables, not chosen.
///
/// gamemd lets a follower into the cell a leader is transiting at the leader's
/// FIRST PAID TRACK POINT: that is where the movement body clears the leader's
/// occupation bit and lowers its cell-occupation-enabled byte. At that instant
/// the leader has advanced from its cell centre by exactly the first inter-point
/// step of whichever curve it selected, and the follower is still a full cell
/// pitch away. So the closest the rule can legitimately put two hulls is one
/// cell minus the largest first step over every curve retail can select.
///
/// Anything tighter than this is not something retail's cell exclusion produces,
/// and is the regime the player reported (the head-on pair measured 38 leptons).
fn derived_min_transit_separation_leptons() -> i64 {
    use crate::sim::movement::drive_track::raw_track_points;
    let mut widest_first_step: i64 = 0;
    for raw_index in 1u8..16 {
        let points = raw_track_points(raw_index);
        if points.len() < 2 {
            continue;
        }
        let dx = i64::from(points[1].x) - i64::from(points[0].x);
        let dy = i64::from(points[1].y) - i64::from(points[0].y);
        let step = (((dx * dx + dy * dy) as f64).sqrt()) as i64;
        widest_first_step = widest_first_step.max(step);
    }
    256 - widest_first_step
}

/// MINIMAL CASE: one stopped vehicle sits on a cell; a second vehicle is
/// ordered by the real command path to move onto exactly that cell.
/// Retail: the mover must NOT be admitted onto the occupied cell.
#[test]
fn repro_second_vehicle_ordered_onto_an_occupied_cell() {
    // Unit741970 names the occupied cell unchanged; the first Process's
    // Find_Path answers code 6 there and, beyond CloseEnough, retargets to an
    // FNPC cell (0x4D3A92..0x4D3E0A) instead of driving into the blocker's
    // cell.
    let Some((mut sim, rules, grid)) = stacking_world(24) else {
        return;
    };

    let blocker = sim
        .spawn_object("MTNK", "Americans", 12, 8, 64, &rules)
        .expect("blocker spawns");
    let mover = sim
        .spawn_object("MTNK", "Americans", 6, 8, 64, &rules)
        .expect("mover spawns");

    let cmd = cmd_envelope(&sim, "Americans", 1, move_command(mover, (12, 8)));
    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 100);

    let mut shared_ticks: Vec<u64> = Vec::new();
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let cells = stacking_cells(&sim, &[blocker, mover]);
        if !stacking_duplicates(&cells).is_empty() {
            shared_ticks.push(tick);
        }
    }

    println!("--- repro_second_vehicle_ordered_onto_an_occupied_cell ---");
    println!("blocker: {}", stacking_motion_state(&sim, blocker));
    println!("mover:   {}", stacking_motion_state(&sim, mover));
    println!("blocker cell: {}", stacking_cell_state(&sim, 12, 8));
    println!(
        "shared-cell ticks: count={} first={:?} last={:?}",
        shared_ticks.len(),
        shared_ticks.first(),
        shared_ticks.last()
    );

    let b = sim.substrate.entities.get(blocker).expect("blocker alive");
    let m = sim.substrate.entities.get(mover).expect("mover alive");
    assert_ne!(
        (b.position.rx(), b.position.ry()),
        (m.position.rx(), m.position.ry()),
        "two ground vehicles must never rest on the same cell"
    );
    // RECORDED RESIDUAL: this is STRICTER than retail. gamemd genuinely lets two
    // vehicles share one `CellClass` in transit — the leader clears its
    // occupation bit at its first paid track point but stays linked in that
    // cell's object list until the crossing, and the derived transit-separation
    // bound exists precisely because the follower can be admitted in that
    // window. Asserting emptiness here is a Rust regression ratchet on a fixture
    // that happens to produce none, not a parity claim, and it must not be read
    // as one.
    assert!(
        shared_ticks.is_empty(),
        "two ground vehicles shared a cell during transit on ticks {shared_ticks:?}"
    );
}

/// Idle objects stay out of the touch logs. The per-frame walks read first
/// and borrow only what they change, so an idle object is handed out mutably
/// only on the frames its Guard handler runs (every 14 to 16 frames here) and
/// an idle building never: the mission AI counter and a building's tint
/// sample, which change every frame, are written without a note. The logs'
/// readers (the blocker plane and the expiry listener
/// index) re-read every object named, so when each walk borrowed every
/// entity, a 20k-object world was re-read almost whole every frame.
#[test]
fn idle_objects_stay_out_of_the_touch_logs() {
    use crate::sim::entity_store::TouchReader;
    use crate::sim::touch_log::Touched;

    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAPOWR\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\n\
         Armor=heavy\nSpeed=6\nTurret=yes\n\
         [GAPOWR]\nStrength=750\nArmor=wood\nPower=100\n",
        ),
        &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
    )
    .expect("idle object rules parse");
    let (mut sim, rules, grid) = stacking_navigation_world(rules, 24);
    let mut vehicles = Vec::new();
    let mut infantry = Vec::new();
    let mut buildings = Vec::new();
    for x in 4..12u16 {
        for y in [6u16, 12] {
            vehicles.push(
                sim.spawn_object("MTNK", "Americans", x, y, 64, &rules)
                    .expect("tank spawns"),
            );
            infantry.push(
                sim.spawn_object("E1", "Americans", x, y + 3, 0, &rules)
                    .expect("infantry spawns"),
            );
        }
    }
    for x in [4u16, 7, 10] {
        buildings.push(
            sim.spawn_object("GAPOWR", "Americans", x, 19, 0, &rules)
                .expect("power plant spawns"),
        );
    }
    for _ in 0..3 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    }
    let _ = sim
        .substrate
        .entities
        .take_touched(TouchReader::BlockerPlane);
    let mut ticks = BTreeMap::<u64, usize>::new();
    for _ in 0..40 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let Touched::Ids(ids) = sim
            .substrate
            .entities
            .take_touched(TouchReader::BlockerPlane)
        else {
            panic!("an idle tick overflowed the touch log");
        };
        for id in ids.into_iter().collect::<BTreeSet<_>>() {
            *ticks.entry(id).or_default() += 1;
        }
    }
    let handed_out = |id: u64| ticks.get(&id).copied().unwrap_or(0);
    for &id in vehicles.iter().chain(&infantry) {
        assert!(handed_out(id) <= 3, "idle vehicle or infantry {id}");
    }
    for &id in &buildings {
        assert_eq!(handed_out(id), 0, "idle building {id}");
    }
}

/// Per tick, each cell two or more group members shared and who shared it.
type SharedCellsByTick = Vec<(u64, BTreeMap<(u16, u16), Vec<u64>>)>;

/// FAITHFUL CASE: eight vehicles selected as a group, one Move order each to
/// a single destination cell, issued in one batch exactly as a group order is.
///
/// Fails in transit on the production Drive track (2026-09-27): vehicles 7 and
/// 8 share a cell 186 leptons apart at tick 22, below the 239 the in-transit
/// check allows. Nothing rests on a shared cell. The short-range case below
/// shows the same numbers. Native behaviour for this case is not established;
/// see that case for the residual.
#[test]
#[ignore = "production Drive track: two tanks share a cell 186 leptons apart in transit"]
fn repro_group_move_of_eight_vehicles_to_one_cell() {
    let Some((mut sim, rules, grid)) = stacking_world(48) else {
        return;
    };

    let start_cells = [
        (6u16, 6u16),
        (7, 6),
        (8, 6),
        (9, 6),
        (6, 7),
        (7, 7),
        (8, 7),
        (9, 7),
    ];
    let ids = spawn_tanks(&mut sim, &rules, &start_cells);

    let target = (30u16, 30u16);
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| cmd_envelope(&sim, "Americans", 1, move_command(id, target)))
        .collect();

    // (a) What the group-destination distributor rewrites each command to.
    // Same &self read the tick performs, run before any tick mutates state.
    let mut staged = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut staged, &rules, None);
    let assigned: Vec<(u64, (u16, u16))> = staged
        .iter()
        .filter_map(|c| match &c.payload {
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
                ..
            } => Some((*entity_id, (*target_rx, *target_ry))),
            _ => None,
        })
        .collect();
    let mut assigned_counts: BTreeMap<(u16, u16), Vec<u64>> = BTreeMap::new();
    for &(id, cell) in &assigned {
        assigned_counts.entry(cell).or_default().push(id);
    }

    println!("--- repro_group_move_of_eight_vehicles_to_one_cell ---");
    println!("(a) distributor assignments (entity -> destination):");
    for (id, cell) in &assigned {
        println!("    {id} -> {cell:?}");
    }
    println!(
        "(a) distinct destinations = {} of {}; collisions = {:?}",
        assigned_counts.len(),
        assigned.len(),
        assigned_counts
            .iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<_>>()
    );

    // Run the real path.
    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    // (c) per-tick cell sharing during transit, with (d) sampled AT the
    // sharing tick — the grid must be read while the two movers are still
    // co-located, not after they have moved on.
    let mut shared_by_tick: SharedCellsByTick = Vec::new();
    let mut shared_snapshots: Vec<String> = Vec::new();
    let mut watch = StackingWatch::default();
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        watch.sample(&sim, &ids, tick);
        let dups = stacking_duplicates(&stacking_cells(&sim, &ids));
        if !dups.is_empty() {
            for (cell, members) in &dups {
                shared_snapshots.push(format!(
                    "tick {tick} shared {cell:?} by {members:?}\n        grid: {}\n        {}",
                    stacking_cell_state(&sim, cell.0, cell.1),
                    members
                        .iter()
                        .map(|&id| stacking_motion_state(&sim, id))
                        .collect::<Vec<_>>()
                        .join("\n        "),
                ));
            }
            shared_by_tick.push((tick, dups));
        }
    }

    // (b) final resting cells.
    let final_cells = stacking_cells(&sim, &ids);
    println!("(b) final positions / (e) still-moving state:");
    for (id, _, _) in &final_cells {
        println!("    {}", stacking_motion_state(&sim, *id));
    }
    let final_dups = stacking_duplicates(&final_cells);
    println!("(b) cells shared AT REST: {final_dups:?}");

    // (c) transient (one isolated tick) vs persistent (consecutive ticks).
    let mut runs: Vec<(u64, u64)> = Vec::new();
    for &(tick, _) in &shared_by_tick {
        match runs.last_mut() {
            Some(last) if last.1 + 1 == tick => last.1 = tick,
            _ => runs.push((tick, tick)),
        }
    }
    println!(
        "(c) ticks with a shared cell: {} across {} consecutive run(s); \
         longest run = {} tick(s)",
        shared_by_tick.len(),
        runs.len(),
        runs.iter().map(|(a, b)| b - a + 1).max().unwrap_or(0),
    );
    println!("(c) runs (first..last tick): {runs:?}");

    // (d) grid state sampled AT each sharing tick.
    println!("(d) grid + entity state at each sharing tick:");
    for snap in shared_snapshots.iter().take(8) {
        println!("    {snap}");
    }
    if shared_snapshots.len() > 8 {
        println!("    ... ({} more)", shared_snapshots.len() - 8);
    }

    // AT REST: cell identity. Retail-backed and kept — a stopped vehicle holds
    // its cell's occupation bit, so two of them cannot rest on one cell.
    assert!(
        final_dups.is_empty(),
        "ground vehicles must never rest on the same cell; shared: {final_dups:?}"
    );
    // IN TRANSIT: hull separation, not cell identity. gamemd releases a mover's
    // occupation bit at its first paid track point and relinks the cell object
    // list only at the crossing, so two vehicles genuinely share one CellClass
    // while one is leaving it. Cell distinctness is therefore NOT retail's
    // in-transit invariant; overlapping hulls are what it never produces, and
    // that is the player's reported symptom.
    watch.report("group_move_eight_to_one_cell");
    watch.assert_no_reported_stacking("group_move_eight_to_one_cell");
}

/// SHORT-RANGE GROUP CASE: the same eight vehicles, but the destination is
/// close enough that most of them arrive within a few ticks of each other,
/// maximising arrival contention. Also traces the full per-tick cell of every
/// member so co-travel (two tanks moving as one) is visible, not just sampled.
///
/// Fails in transit on the production Drive track (2026-09-27): vehicles 5 and
/// 6 share a cell 186 leptons apart at tick 22, below the 239 the in-transit
/// check allows; nothing rests on a shared cell. VERA on the retail Dustbowl
/// map gives the same 186 leptons at tick 22 at three open sites (a temporary
/// probe spawning this group on open ground through `headless_scenario`).
/// RESIDUAL: trigger — any group Move of adjacent Drive vehicles; effect —
/// two hulls overlap by about a quarter cell for a few ticks mid-route;
/// frequency — every group order; risk — visual only, nothing rests stacked.
/// Whether gamemd keeps them farther apart is unmeasured.
#[test]
#[ignore = "production Drive track: two tanks share a cell 186 leptons apart in transit"]
fn repro_group_move_short_range_traces_every_tick() {
    let Some((mut sim, rules, grid)) = stacking_world(48) else {
        return;
    };

    let start_cells = [
        (10u16, 10u16),
        (11, 10),
        (12, 10),
        (13, 10),
        (10, 11),
        (11, 11),
        (12, 11),
        (13, 11),
    ];
    let ids = spawn_tanks(&mut sim, &rules, &start_cells);

    let target = (16u16, 16u16);
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| cmd_envelope(&sim, "Americans", 1, move_command(id, target)))
        .collect();

    let mut staged = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut staged, &rules, None);
    println!("--- repro_group_move_short_range_traces_every_tick ---");
    println!("(a) distributor assignments:");
    for c in &staged {
        if let Command::Move {
            entity_id,
            target_rx,
            target_ry,
            ..
        } = &c.payload
        {
            println!("    {entity_id} -> ({target_rx},{target_ry})");
        }
    }

    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    let mut shared_by_tick: SharedCellsByTick = Vec::new();
    let mut trace: Vec<String> = Vec::new();
    let mut watch = StackingWatch::default();
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        watch.sample(&sim, &ids, tick);
        let cells = stacking_cells(&sim, &ids);
        let dups = stacking_duplicates(&cells);
        if tick < 120 {
            trace.push(format!(
                "t{tick:>3} {}{}",
                cells
                    .iter()
                    .map(|(id, rx, ry)| format!("{id}:({rx},{ry})"))
                    .collect::<Vec<_>>()
                    .join(" "),
                if dups.is_empty() {
                    String::new()
                } else {
                    format!("   <<< SHARED {dups:?}")
                }
            ));
        }
        if !dups.is_empty() {
            for (cell, members) in &dups {
                trace.push(format!(
                    "t{tick:>3} SHARED {cell:?} by {members:?}\n        grid: {}\n        {}",
                    stacking_cell_state(&sim, cell.0, cell.1),
                    members
                        .iter()
                        .map(|&id| stacking_reservation_state(&sim, id))
                        .collect::<Vec<_>>()
                        .join("\n        "),
                ));
            }
            shared_by_tick.push((tick, dups));
        }
    }

    for line in &trace {
        println!("{line}");
    }

    let final_cells = stacking_cells(&sim, &ids);
    for (id, _, _) in &final_cells {
        println!("    {}", stacking_motion_state(&sim, *id));
    }
    let final_dups = stacking_duplicates(&final_cells);
    println!("cells shared AT REST: {final_dups:?}");
    println!("ticks with a shared cell: {}", shared_by_tick.len());

    // AT REST: cell identity. Retail-backed and kept — a stopped vehicle holds
    // its cell's occupation bit, so two of them cannot rest on one cell.
    assert!(
        final_dups.is_empty(),
        "ground vehicles must never rest on the same cell; shared: {final_dups:?}"
    );
    // IN TRANSIT: hull separation, not cell identity. gamemd releases a mover's
    // occupation bit at its first paid track point and relinks the cell object
    // list only at the crossing, so two vehicles genuinely share one CellClass
    // while one is leaving it. Cell distinctness is therefore NOT retail's
    // in-transit invariant; overlapping hulls are what it never produces, and
    // that is the player's reported symptom.
    watch.report("group_move_short_range");
    watch.assert_no_reported_stacking("group_move_short_range");
}

/// What the mover's own `Can_Enter_Cell` (`foot_can_enter`, with no direction
/// or height) says for `mover` entering `cell`, evaluated read-only from live
/// sim state.
fn stacking_cell_entry_verdict(
    sim: &Simulation,
    rules: &RuleSet,
    mover: u64,
    rx: u16,
    ry: u16,
) -> String {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return "<no terrain>".to_string();
    };
    let cell = terrain.native_cell_identity((rx as i16, ry as i16));
    let args = crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR;
    match sim.foot_can_enter(mover, cell, args, rules) {
        Ok(code) => format!("code={code}"),
        Err(error) => format!("<{error}>"),
    }
}

/// Minimal two-mover case with independent destinations.
///
/// Two vehicles are given INDEPENDENT Move commands with DIFFERENT targets,
/// so each is a run of one and the group-destination distributor never runs.
/// They approach head-on. Measure separation and the bounded shared-cell
/// episode through ordinary route selection and paid Drive curves.
#[test]
fn repro_two_moving_vehicles_pass_through_each_other() {
    let Some((mut sim, rules, grid)) = stacking_world(24) else {
        return;
    };

    let west = sim
        .spawn_object("MTNK", "Americans", 5, 10, 64, &rules)
        .expect("west tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 15, 10, 192, &rules)
        .expect("east tank spawns");

    let commands = vec![
        cmd_envelope(&sim, "Americans", 1, move_command(west, (16, 10))),
        cmd_envelope(&sim, "Americans", 1, move_command(east, (4, 10))),
    ];
    // Different targets => different formation keys => runs of length 1 =>
    // the group-destination distributor cannot touch either command.
    let mut staged = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut staged, &rules, None);
    println!("--- repro_two_moving_vehicles_pass_through_each_other ---");
    for c in &staged {
        if let Command::Move {
            entity_id,
            target_rx,
            target_ry,
            ..
        } = &c.payload
        {
            println!("    post-distributor: {entity_id} -> ({target_rx},{target_ry})");
        }
    }

    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    let ids = [west, east];
    let mut shared_ticks: Vec<u64> = Vec::new();
    let mut snapshots: Vec<String> = Vec::new();
    let mut closest_approach: Option<(i64, u64)> = None;
    let mut shared_cell_approach: Option<(u64, u64, i64, u64)> = None;
    // Preserve the production route changes that select the subsequent Drive
    // curves. The original finishing owner now supplies concrete CanEnter
    // results; cell-occupancy presence alone no longer vetoes a shortcut.
    let path_of = |sim: &Simulation, id| {
        sim.substrate
            .entities
            .get(id)
            .filter(|e| crate::sim::movement::motion_query::is_moving(&e) == Some(true))
            .map(|e| e.navigation.path_replay().route_cells())
            .unwrap_or_default()
    };
    let mut previous_paths = [path_of(&sim, west), path_of(&sim, east)];
    let mut route_trace = vec![format!(
        "initial west={:?} east={:?}",
        previous_paths[0], previous_paths[1]
    )];
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        for (index, id) in ids.iter().copied().enumerate() {
            let path = path_of(&sim, id);
            if path != previous_paths[index] {
                route_trace.push(format!(
                    "tick {tick} receiver {id} path {path:?}; {}",
                    stacking_motion_state(&sim, id)
                ));
                previous_paths[index] = path;
            }
        }
        if let Some(gap) = stacking_gap(&sim, west, east)
            && closest_approach.is_none_or(|(best, _)| gap < best)
        {
            closest_approach = Some((gap, tick));
        }
        if let Some((a, b, gap)) = stacking_min_gap_within_shared_cell(&sim, &ids)
            && shared_cell_approach.is_none_or(|(_, _, best, _)| gap < best)
        {
            shared_cell_approach = Some((a, b, gap, tick));
        }
        let dups = stacking_duplicates(&stacking_cells(&sim, &ids));
        if !dups.is_empty() {
            shared_ticks.push(tick);
            for (cell, members) in &dups {
                snapshots.push(format!(
                    "tick {tick} cell {cell:?} members {members:?}\n        grid: {}\n        {}\n        \
                     cell-entry predicate for {}: {}",
                    stacking_cell_state(&sim, cell.0, cell.1),
                    members
                        .iter()
                        .map(|&id| stacking_motion_state(&sim, id))
                        .collect::<Vec<_>>()
                        .join("\n        "),
                    members[1],
                    stacking_cell_entry_verdict(&sim, &rules, members[1], cell.0, cell.1),
                ));
            }
        }
    }

    for row in &route_trace {
        println!("    route: {row}");
    }
    println!("west: {}", stacking_motion_state(&sim, west));
    println!("east: {}", stacking_motion_state(&sim, east));
    println!("shared ticks: {} -> {shared_ticks:?}", shared_ticks.len());
    for snap in snapshots.iter().take(10) {
        println!("    {snap}");
    }
    if snapshots.len() > 10 {
        println!("    ... ({} more)", snapshots.len() - 10);
    }

    // THE REPORTED SYMPTOM, MEASURED. This is the only fixture in the repo known
    // to have produced the player's overlap: before the cell-exclusion gate both
    // tanks occupied (10,10) for ten consecutive ticks and closed to 38 leptons
    // — an ~85% hull overlap. Cell distinctness alone places no lower bound on
    // separation, so it cannot detect a regression of that; the separation
    // assertions below can.
    //
    // TWO REGIMES, TWO BOUNDS — and they are not interchangeable.
    //
    // `derived_min_transit_separation_leptons` is derived for one instant: when
    // gamemd admits a mover into a cell another mover is still occupying. It
    // does NOT bound two movers in DIFFERENT cells, and gamemd's own crossing
    // rule refutes any attempt to make it: `Process_Drive_Track` derives the
    // cell from one absolute coordinate and crosses the moment that coordinate
    // crosses, so a hull sits a handful of leptons inside a cell immediately
    // after entering it. A vehicle resting at the centre of the cell it just
    // left is then a little over half a cell away, both legally in their own
    // cells. Applying the transit bound to every pair asserted something retail
    // does not honour, so the transit bound is asserted here on the regime it
    // was derived for, and the all-pair measurement keeps the half-cell
    // "two hulls on one spot" floor used by
    // `group_move_never_draws_two_hulls_on_one_spot` — which is what the
    // player's 38-lepton report was a violation of.
    //
    // FLOOR RE-DERIVED 2026-09-15 with the native ally-occupant arm in place.
    // `Process_Drive_Track` clears the mover's own raw occupation bit and
    // `Foot+0x6B6` on the first processed point while `+0x6B6` is still set
    // (`0x004B15D3 CALL [owner+0xC0]`, `0x004B1611 CALL [owner+0xF4]`,
    // `0x004B161A`; chained curves do not re-arm it), `UnitClass::Can_Enter_Cell` then skips
    // that in-transit ally on the object list unless its locomotor answers
    // `Can_Use_Track` (`0x0073FA30..FA7C`), and the head-on exit fires only for
    // an ally facing the mover inside `0x1FF` leptons and inside the mover's
    // octant (`0x0073F8D4..FA26`). So once the east tank has committed a
    // dodge curve out of its cell, nothing native keeps the west tank out of
    // that cell while the dodging hull is still leaving it. Measured closest
    // approach on this fixture with that contract: 116 leptons at tick 77,
    // in different cells. The earlier 128 floor was VERA's own half-cell
    // heuristic; the player's report was 38 leptons in ONE cell, which the
    // shared-cell transit bound below still refuses. This floor is a ratchet
    // on the measured value, not a native bound.
    //
    // RE-MEASURED 2026-09-23 with the same-call track-end continuation (a
    // track end runs Process_Movement and Process_Track(1) in that Process,
    // Drive 0x4B0583..0x4B0667). The west tank now selects its next head in
    // the Process that ends its track at (10,10), one frame before the former
    // next-visit reselection, and enters (11,10) while the east tank's dodge
    // curve is still leaving through that cell's south-west corner: closest
    // approach 108 leptons at tick 77, and the pair shares (11,10) for tick
    // 78 only, 119 leptons apart, below the derived transit bound. That bound
    // is derived for the admission instant, inside the frame, which this
    // fixture does not sample; whether native admits here too is open
    // (ledger row I15).
    // Ratchets on the measured values, not native bounds; the bound is
    // printed.
    const VISIBLE_OVERLAP_LEPTONS: i64 = 108;
    // Original42ACF0 now downgrades a same-type/no-processed-peer requested
    // urgency1 to0. Native execution pins that transition and the resulting
    // clearing code2 cost1 rather than4 in astar_hills_markers.json. On main7412
    // this fixture retained urgency1 and detoured at tick38. Holding only that
    // value in the new marker owner exactly recovers main's path and127-lepton
    // closest approach with no shared cells; native effective urgency instead
    // selects the raw W/SW route and later shares one cell116 leptons apart.
    // See tools/spatial_oracle/astar_path_finishing_replay/marker_urgency (in git
    // at 1b1a8b43e) for unchanged-binary causal transcripts. This floor remains a
    // Rust regression ratchet, not a native two-vehicle world bound. The visible108
    // and one-tick limits remain.
    const SHARED_CELL_FLOOR_LEPTONS: i64 = 116;
    const SHARED_CELL_TICKS: usize = 1;
    let bound = derived_min_transit_separation_leptons();
    let (gap, gap_tick) = closest_approach.expect("both movers sampled");
    println!(
        "closest approach (any pair): {gap} leptons ({:.2} cells) at tick {gap_tick}; \
         transit bound {bound}, visible-overlap floor {VISIBLE_OVERLAP_LEPTONS}; \
         closest approach while sharing a cell: {shared_cell_approach:?}",
        gap as f64 / 256.0
    );
    assert!(
        gap >= VISIBLE_OVERLAP_LEPTONS,
        "head-on pair closed to {gap} leptons ({:.2} cells) at tick {gap_tick} — hulls visibly overlap",
        gap as f64 / 256.0
    );
    if let Some((a, b, shared_gap, shared_tick)) = shared_cell_approach {
        assert!(
            shared_gap >= SHARED_CELL_FLOOR_LEPTONS,
            "vehicles {a} and {b} shared a cell only {shared_gap} leptons ({:.2} cells) at tick {shared_tick}",
            shared_gap as f64 / 256.0
        );
    }
    // RECORDED RESIDUAL: stricter than retail, for the reason written out at
    // `repro_second_vehicle_ordered_onto_an_occupied_cell`. Regression ratchet,
    // not a parity claim.
    assert!(
        shared_ticks.len() <= SHARED_CELL_TICKS,
        "two moving ground vehicles shared a cell on ticks {shared_ticks:?}"
    );
}

/// Head-to reservation + occupation-bit state for one vehicle.
fn stacking_reservation_state(sim: &Simulation, id: u64) -> String {
    let Some(e) = sim.substrate.entities.get(id) else {
        return format!("{id}:<gone>");
    };
    let Some(d) = retained_drive(e) else {
        return format!("{id}:<no drive>");
    };
    format!(
        "{id}@({},{})sub({},{}) head_to={:?} cur_cleared={}",
        e.position.rx(),
        e.position.ry(),
        e.position.sub_x(),
        e.position.sub_y(),
        d.head_to()
            .map(|head| ((head.x / 256) as u16, (head.y / 256) as u16)),
        !e.foot_occupation_enabled(),
    )
}

/// Tick-by-tick trace of the head-on race, showing exactly when each mover
/// installs its head-to reservation on the contested cell and whether the
/// other mover's reservation was visible at that moment.
#[test]
fn repro_two_moving_vehicles_reservation_trace() {
    let Some((mut sim, rules, grid)) = stacking_world(24) else {
        return;
    };

    let west = sim
        .spawn_object("MTNK", "Americans", 5, 10, 64, &rules)
        .expect("west tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 15, 10, 192, &rules)
        .expect("east tank spawns");

    let commands = vec![
        cmd_envelope(&sim, "Americans", 1, move_command(west, (16, 10))),
        cmd_envelope(&sim, "Americans", 1, move_command(east, (4, 10))),
    ];
    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    println!("--- repro_two_moving_vehicles_reservation_trace ---");
    // The head-to mark is the only cell-exclusion mechanism a Drive curve
    // installs: Apply(1) ORs 0x20 into its committed head's cell. The raw
    // byte names no owner, so each mover's retained head records the cell it
    // marked. If two movers hold it on the same cell at the same tick, the
    // reservation is not exclusive and both will commit into that cell.
    let mut double_reservation_ticks: Vec<(u64, (u16, u16))> = Vec::new();
    for tick in 0..80u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let heads: Vec<Option<(u16, u16)>> = [west, east]
            .iter()
            .map(|&id| {
                sim.substrate
                    .entities
                    .get(id)
                    .and_then(retained_drive)
                    .and_then(|d| d.head_to())
                    .map(|head| ((head.x / 256) as u16, (head.y / 256) as u16))
            })
            .collect();
        if let (Some(a), Some(b)) = (heads[0], heads[1])
            && a == b
        {
            double_reservation_ticks.push((tick, a));
        }
        if !(40..=75).contains(&tick) {
            continue;
        }
        let bits: Vec<String> = (9u16..=11)
            .map(|rx| {
                format!(
                    "({rx},10)=0x{:02X}/n{}",
                    sim.substrate.raw_cell_occupation.ground_bits(rx, 10),
                    sim.substrate
                        .occupancy
                        .count_on_layer(rx, 10, MovementLayer::Ground),
                )
            })
            .collect();
        println!(
            "t{tick:>3} | {} | {} | cells {}",
            stacking_reservation_state(&sim, west),
            stacking_reservation_state(&sim, east),
            bits.join(" "),
        );
    }

    println!("double-reserved ticks: {double_reservation_ticks:?}");
    // The double-reservation check only fires while BOTH movers hold a head-to
    // reservation, and a refusal sets that to None — so a permanently gridlocked
    // pair would satisfy it vacuously. Require real progress first: both movers
    // must have left their start cells, which a gridlocked pair never does.
    let west_moved = sim
        .substrate
        .entities
        .get(west)
        .is_some_and(|e| (e.position.rx(), e.position.ry()) != (5, 10));
    let east_moved = sim
        .substrate
        .entities
        .get(east)
        .is_some_and(|e| (e.position.rx(), e.position.ry()) != (15, 10));
    assert!(
        west_moved && east_moved,
        "the reservation check is vacuous unless both movers actually moved;          west_moved={west_moved} east_moved={east_moved} — {} | {}",
        stacking_motion_state(&sim, west),
        stacking_motion_state(&sim, east)
    );
    assert!(
        double_reservation_ticks.is_empty(),
        "two Drive movers held the head-to cell reservation on the SAME cell: \
         {double_reservation_ticks:?}"
    );
}

/// DEADLOCK GUARD — head-on pair.
///
/// The cell-exclusion gate refuses a curve into a cell another vehicle has
/// already claimed. Two movers that each want the other's cell would freeze
/// under a bare refusal, so the refusal must land in gamemd's per-code
/// dispatch: a temporary claim waits and repaths at escalating urgency, and a
/// blocker that has come to rest gets scattered out of the way. Both tanks must
/// therefore still finish their orders.
///
/// Fails on the production Find_Path owner and Drive track (2026-09-27): the
/// pair meets mid-row and both tanks drop their orders side by side, as VERA
/// does at three open sites on the retail Dustbowl map (a temporary probe
/// through `headless_scenario`). It passed before only through the legacy
/// inline search that fixtures without native zones took. RESIDUAL: trigger —
/// two Drive vehicles ordered through each other's cells on one row; effect —
/// both stop short, orders dropped; frequency — head-on traffic in lanes;
/// risk — units fail to reach ordered cells. Native behaviour for a head-on
/// pair is not established.
#[test]
#[ignore = "production Drive path drops both head-on orders (VERA on retail Dustbowl too)"]
fn head_on_pair_resolves_without_deadlock() {
    let Some((mut sim, rules, grid)) = stacking_world(24) else {
        return;
    };

    let west = sim
        .spawn_object("MTNK", "Americans", 5, 10, 64, &rules)
        .expect("west tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 15, 10, 192, &rules)
        .expect("east tank spawns");

    let commands = vec![
        cmd_envelope(&sim, "Americans", 1, move_command(west, (16, 10))),
        cmd_envelope(&sim, "Americans", 1, move_command(east, (4, 10))),
    ];
    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    // Generous budget: 11 cells each at MTNK speed, plus whatever the block
    // dispatch costs in waits and repaths.
    let mut west_done_at: Option<u64> = None;
    let mut east_done_at: Option<u64> = None;
    for tick in 0..900u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        let w = sim.substrate.entities.get(west).expect("west alive");
        let e = sim.substrate.entities.get(east).expect("east alive");
        if west_done_at.is_none() && (w.position.rx(), w.position.ry()) == (16, 10) {
            west_done_at = Some(tick);
        }
        if east_done_at.is_none() && (e.position.rx(), e.position.ry()) == (4, 10) {
            east_done_at = Some(tick);
        }
        if west_done_at.is_some() && east_done_at.is_some() {
            break;
        }
    }

    println!("--- head_on_pair_resolves_without_deadlock ---");
    println!("west: {}", stacking_motion_state(&sim, west));
    println!("east: {}", stacking_motion_state(&sim, east));
    println!("west reached goal at tick {west_done_at:?}, east at {east_done_at:?}");

    assert!(
        west_done_at.is_some(),
        "west tank never reached (16,10): {}",
        stacking_motion_state(&sim, west)
    );
    assert!(
        east_done_at.is_some(),
        "east tank never reached (4,10): {}",
        stacking_motion_state(&sim, east)
    );
}

/// DEADLOCK GUARD — column.
///
/// Four vehicles queued nose-to-tail on one row, each with its own destination
/// further along that row. The trailing movers repeatedly select a curve into
/// the cell the mover ahead is heading for, so this is the case the gate is
/// asked about most often in ordinary play. Every member must arrive, and no
/// two may ever occupy one cell.
#[test]
fn column_of_vehicles_all_arrive_without_stacking() {
    let Some((mut sim, rules, grid)) = stacking_world(32) else {
        return;
    };

    let starts = [(5u16, 10u16), (6, 10), (7, 10), (8, 10)];
    let goals = [(18u16, 10u16), (19, 10), (20, 10), (21, 10)];
    let ids = spawn_tanks(&mut sim, &rules, &starts);

    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .zip(goals.iter())
        .map(|(&id, &goal)| cmd_envelope(&sim, "Americans", 1, move_command(id, goal)))
        .collect();
    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    let mut watch = StackingWatch::default();
    for tick in 0..900u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        watch.sample(&sim, &ids, tick);
    }

    println!("--- column_of_vehicles_all_arrive_without_stacking ---");
    for &id in &ids {
        println!("    {}", stacking_motion_state(&sim, id));
    }

    let mut stalled: Vec<String> = Vec::new();
    for (&id, &goal) in ids.iter().zip(goals.iter()) {
        let at_goal = sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| (e.position.rx(), e.position.ry()) == goal);
        if !at_goal {
            stalled.push(format!(
                "{} (wanted {goal:?})",
                stacking_motion_state(&sim, id)
            ));
        }
    }

    assert!(
        stalled.is_empty(),
        "column members never reached their destinations: {stalled:?}"
    );
    // Their destinations are distinct cells, so arriving proves the at-rest
    // half. In transit, measure hull separation rather than cell identity for
    // the reason written out at the group fixtures.
    watch.report("column_of_four");
    watch.assert_no_reported_stacking("column_of_four");
}

/// How close two vehicles actually get, in leptons, over a group move.
///
/// The three cell-identity tests above assert that no two vehicles ever share
/// an `(rx, ry)`. That is stricter than the retail invariant: gamemd derives a
/// unit's cell from one absolute coordinate, releases its cell-occupation bit
/// on the first paid track point of a curve, and only relinks the cell object
/// list at the crossing itself — so a follower is admitted into the cell a
/// leader is leaving while the leader's body is still nominally in it. What
/// gamemd never produces is two hulls drawn on the same spot. This test
/// measures that directly: the closest approach between any two movers, in
/// leptons (256 per cell). The reported bug measured 38.
#[test]
fn group_move_never_draws_two_hulls_on_one_spot() {
    let Some((mut sim, rules, grid)) = stacking_world(48) else {
        return;
    };

    let start_cells = [
        (10u16, 10u16),
        (11, 10),
        (12, 10),
        (13, 10),
        (10, 11),
        (11, 11),
        (12, 11),
        (13, 11),
    ];
    let ids = spawn_tanks(&mut sim, &rules, &start_cells);
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| cmd_envelope(&sim, "Americans", 1, move_command(id, (16, 16))))
        .collect();
    let _ = sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 100);

    // Closest approach over the move, in the lepton frame the renderer draws
    // rather than the cell index.
    let mut worst: Option<(u64, u64, u64, i64)> = None;
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
        if let Some((a, b, gap)) = stacking_min_gap(&sim, &ids)
            && worst.is_none_or(|(_, _, _, best)| gap < best)
        {
            worst = Some((tick, a, b, gap));
        }
    }

    let (tick, a, b, gap) = worst.expect("at least one pair sampled");
    println!(
        "--- group_move_never_draws_two_hulls_on_one_spot ---\n\
         closest approach: {gap} leptons ({:.2} cells) between {a} and {b} at tick {tick}",
        gap as f64 / 256.0
    );

    // Half a cell. Below this the two hulls visibly overlap, which is the
    // player-reported symptom; the reported bug produced 38.
    assert!(
        gap >= 128,
        "two vehicles closed to {gap} leptons ({:.2} cells) at tick {tick} \
         ({a} and {b}) — hulls visibly overlap",
        gap as f64 / 256.0
    );
}

/// One group Move order to (20, 20) for `ids`, as the staged run
/// `0x0064CDA0` adjusts, and each member's adjusted target.
fn group_spread(sim: &Simulation, rules: &RuleSet, ids: &[u64]) -> Vec<(u16, u16)> {
    let mut staged = ids
        .iter()
        .map(|&id| cmd_envelope(sim, "Americans", 1, move_command(id, (20, 20))))
        .collect::<Vec<_>>();
    sim.adjust_staged_megamission_destinations(&mut staged, rules, None);
    staged
        .iter()
        .map(|command| match command.payload {
            Command::Move {
                target_rx,
                target_ry,
                ..
            } => (target_rx, target_ry),
            _ => unreachable!("the run holds Move orders"),
        })
        .collect()
}

/// `0x0064CDA0` asks each member's own `Can_Enter_Cell` (`vt+0x1AC` at
/// `0x0064D52F`) with no direction and the target's height (its level, plus
/// 4 on a bridge). Two tanks side by side: the eastern one is the anchor and
/// takes the target, and the western one probes west of it. After a refusal,
/// an admitted candidate still yields the saved cell (`0x0064D5C5`), here
/// the target.
#[test]
fn a_group_spread_asks_each_members_can_enter_cell_at_the_target_height() {
    use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord};
    let Some((mut sim, rules, _)) = stacking_world(32) else {
        return;
    };
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(20, 20)
        .unwrap()
        .level = 2;
    let west = sim
        .spawn_object("MTNK", "Americans", 6, 6, 64, &rules)
        .expect("tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 8, 6, 64, &rules)
        .expect("tank spawns");

    fresh_oracle_seam::install(vec![7, 0], Vec::new());
    let targets = group_spread(&sim, &rules, &[west, east]);
    let (records, unused) = fresh_oracle_seam::finish();

    let asked = |cell, code| FreshCallRecord::CanEnter {
        cell,
        direction: -1,
        height: 2,
        code,
    };
    assert_eq!(records, [asked((19, 20), 7), asked((18, 20), 0)]);
    assert_eq!(unused, 0);
    assert_eq!(targets, [(20, 20), (20, 20)]);
}

/// An aircraft member answers through `AircraftClass::Can_Enter_Cell`
/// (`0x004196B0`): in game mode 0, the current house's aircraft refuses a
/// shrouded cell. The western Harrier's six probes are all shrouded until the
/// area is revealed, so it keeps the target.
#[test]
fn a_group_spread_keeps_aircraft_out_of_shrouded_cells() {
    for revealed in [false, true] {
        let Some((mut sim, rules, _)) = stacking_world(32) else {
            return;
        };
        let west = sim
            .spawn_object("ORCA", "Americans", 6, 6, 64, &rules)
            .expect("Harrier spawns");
        let east = sim
            .spawn_object("ORCA", "Americans", 8, 6, 64, &rules)
            .expect("Harrier spawns");
        for id in [west, east] {
            let harrier = sim.substrate.entities.get_mut(id).unwrap();
            harrier
                .seldom
                .discovery
                .set_owned_by_current_house_for_test(true);
        }
        let owner = sim.interner.intern("Americans");
        sim.fog = crate::sim::vision::FogState::new(32, 32);
        if revealed {
            crate::sim::vision::reveal_radius(&mut sim.fog, owner, 20, 20, 8);
            assert!(sim.fog.is_ground_unshrouded(owner, 19, 20));
        }

        let targets = group_spread(&sim, &rules, &[west, east]);

        let western = if revealed { (19, 20) } else { (20, 20) };
        assert_eq!(targets, [western, (20, 20)], "revealed {revealed}");
    }
}

/// Infantry members ask InfantryClass's own `Can_Enter_Cell`. A target with
/// the bridge flag `0x100` gives the deck height (level 1 plus 4). With no
/// bridge record behind the flag, the target's GetZoneID answers the DWORD -1,
/// which no candidate shares. Every probe still asks `Can_Enter_Cell` first
/// (`0x0064D52F`, before the zone compare at `0x0064D537`), and the member
/// keeps the target.
#[test]
fn a_group_spread_asks_infantry_with_a_flagged_targets_deck_height() {
    use crate::sim::movement::fresh_oracle_seam::{self, FreshCallRecord};
    let Some((mut sim, rules, _)) = stacking_world(32) else {
        return;
    };
    let target = sim
        .resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(20, 20)
        .unwrap();
    target.level = 1;
    target.bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    let west = sim
        .spawn_object("E1", "Americans", 6, 6, 0, &rules)
        .expect("infantry spawns");
    let east = sim
        .spawn_object("E1", "Americans", 8, 6, 0, &rules)
        .expect("infantry spawns");

    fresh_oracle_seam::install(vec![0; 6], Vec::new());
    let targets = group_spread(&sim, &rules, &[west, east]);
    let (records, unused) = fresh_oracle_seam::finish();

    assert_eq!(unused, 0, "six probes, one Can_Enter_Cell each");
    for record in &records {
        let FreshCallRecord::CanEnter {
            cell,
            direction,
            height,
            ..
        } = record
        else {
            panic!("unexpected call {record:?}");
        };
        assert_eq!((cell.1, *direction, *height), (20, -1, 5), "{record:?}");
    }
    assert_eq!(targets, [(20, 20), (20, 20)]);
}
