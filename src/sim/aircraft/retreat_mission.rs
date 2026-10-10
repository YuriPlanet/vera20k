//! `AircraftClass::Mission_Retreat @ 0x00415A50`, the Retreat handler
//! (vt+0x230, jump table `0x005B34E8` entry 4). A missile takes it when it
//! loses its target (`aircraft::idle_entry`) and keeps it: Queue_Mission
//! refuses another mission over Retreat (`0x0041BA90`).
//!
//! - Without a NavCom (`+0x5A4`): a cell on the house's edge, its `Edge=`
//!   (`HouseClass+0x1E0`) when 0..3, else its waypoint edge (`0x0050DA80`),
//!   from `MapClass::PickCellOnEdge @ 0x004AA440` (the empty cell
//!   `0x00889E68` as both references, criterion 4) and, unless that is the
//!   empty cell itself (both words zero), the destination
//!   (`MapClass::operator[] @ 0x005657A0`, vt+0x480(cell, 1)).
//! - With the NavCom the cell the aircraft is over (GetCell, vt+0x1BC,
//!   `0x005F6960`): no destination (vt+0x480(NULL, 1)).
//!
//! Every visit returns 3 frames.
//!
//! Evidence: tools/spatial_oracle/aircraft_idle_retreat.py runs the original
//! over each `Edge=` and waypoint edge, picked cells equal to and differing
//! from the empty cell, and each NavCom kind; `retreat_mission_tests`
//! replays every row through [`retreat_visit`]. PickCellOnEdge's search and
//! draws are aircraft_states.py's (Mission_Attack's state 10 calls it the
//! same way).
//!

use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::CellRef;
use crate::sim::components::NavTargetRef;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::sim::world::edge_cell::Edge;

#[cfg(test)]
#[path = "retreat_mission_tests.rs"]
mod tests;

/// Every visit's return (`0x00415AD5`, `0x00415AFF`).
const RETREAT_FRAMES: i32 = 3;

/// The NavCom against the cell the aircraft is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetreatNavCom {
    None,
    OwnCell,
    Elsewhere,
}

/// What a visit does beyond its own reads.
trait RetreatHost {
    /// PickCellOnEdge's cell on `edge`.
    fn edge_cell(&mut self, edge: Edge) -> Option<(u16, u16)>;
    /// vt+0x480 `Assign_Destination(cell, 1)`, NULL for `None`.
    fn assign_destination(&mut self, cell: Option<(u16, u16)>);
}

/// One visit, given the NavCom, the house's `Edge=` (`+0x1E0`) and its
/// waypoint edge (`+0x577C`).
fn retreat_visit(
    nav_com: RetreatNavCom,
    house_edge: i32,
    waypoint_edge: u8,
    host: &mut impl RetreatHost,
) -> i32 {
    match nav_com {
        RetreatNavCom::None => {
            let edge = Edge::authored_or_waypoint(house_edge, waypoint_edge);
            if let Some(cell) = host.edge_cell(edge).filter(|&cell| cell != (0, 0)) {
                host.assign_destination(Some(cell));
            }
        }
        RetreatNavCom::OwnCell => host.assign_destination(None),
        RetreatNavCom::Elsewhere => {}
    }
    RETREAT_FRAMES
}

/// One visit of aircraft `id`.
pub(super) fn retreat(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    frame_effects: FrameEffects<'_>,
) -> i32 {
    let entity = sim.substrate.entities.get(id).expect("aircraft dispatch");
    let nav_com = match entity.navigation.nav_com {
        None => RetreatNavCom::None,
        Some(nav_com) => {
            // GetCell: Map[coord] of the Location, whose miss is the dummy
            // cell no NavCom is.
            let here = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let own = crate::sim::cell_rect::get_cellclass_fallback_leptons(
                sim.resolved_terrain.as_ref(),
                here.x,
                here.y,
            );
            match (nav_com, own) {
                (NavTargetRef::Cell { rx, ry }, CellRef::Real(cell))
                    if (cell.rx, cell.ry) == (rx, ry) =>
                {
                    RetreatNavCom::OwnCell
                }
                _ => RetreatNavCom::Elsewhere,
            }
        }
    };
    let waypoint_edge = sim.aircraft_house_waypoint_edge(id);
    let house_edge = sim
        .houses
        .get(&entity.owner())
        .map_or(-1, |house| house.authored_edge());
    retreat_visit(
        nav_com,
        house_edge,
        waypoint_edge,
        &mut WorldRetreat {
            sim,
            id,
            rules,
            frame_effects,
        },
    )
}

struct WorldRetreat<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    frame_effects: FrameEffects<'a>,
}

impl RetreatHost for WorldRetreat<'_> {
    fn edge_cell(&mut self, edge: Edge) -> Option<(u16, u16)> {
        self.sim.aircraft_edge_cell(edge)
    }

    fn assign_destination(&mut self, cell: Option<(u16, u16)>) {
        self.sim.assign_aircraft_destination(
            self.id,
            cell.map(|(rx, ry)| NavTargetRef::cell(rx, ry)),
            self.rules,
            self.frame_effects,
        );
    }
}
