//! `PathFound` and `ApproachPoint` (RFC 0001 appendix 4.5, preview): a
//! module's say over the route a Find_Path search hands a Foot, and over
//! the cell a Foot approaching its target heads for.
//!
//! ## `PathFound`
//!
//! Where: `FootClass::Find_Path @ 0x004D3920`'s core
//! (`movement::foot_path`, `search_foot_path`), once the search answered
//! and before its route is copied into the Foot's path queue
//! (`0x004D3E82..0x004D3E9F`): Drive, Ship, Walk and Hover requests, a
//! train's follower cars included. The Foot is lifted (Mark0) while it is
//! asked, as during the search. Not asked: a train extending its last queued
//! step (a nonzero prefix, `retain_head`), a search that stopped for want of
//! map data, and a route with a tube step (direction 8).
//!
//! `subject` the Foot, `x`, `y`, `z` its position, `cell` the search goal's
//! cell; `native` (and `value`) the number of cells of the native route, 0
//! when it found none. The result area holds up to 24 `PathCell`s (the
//! Find_Path window), the first link getting the native route's cells (up to
//! 24) from the cell after the Foot's; `result` is the number of cells
//! answered. 0 is no route: the search's failure arm runs (PathDelay,
//! Stop_Driver, the nearby-cell redirect), unless the native route was empty
//! too. 1..24 cells replace the route: each must be on the map and next to
//! the one before (the first next to the Foot's cell); a last accepted
//! answer that breaks this keeps the native route. The engine takes the
//! cells as the route's directions
//! from the Foot's cell; its locomotor then drives them as a native route
//! (Can_Enter_Cell for each next cell, a new request when they run out).
//!
//! The path batch (`movement::path_batch`, MOV-Q9): it stays on for bound
//! Foots. The decision is not made inside the batch's side-by-side search
//! (no module runs on a worker thread): the batch only computes the native
//! answer early, and the Foot adopts it in its own turn, where it is asked.
//! So a bound Foot's requests wait for the batch as any Foot's do, and the
//! frame order is the same with and without a decider; a module cannot skip
//! the native search, whose work stays (it draws no random numbers, so the
//! checklist's 4.4 rule holds without extra draws).
//!
//! Residuals:
//! - The answer is the route's first window only: a long trip asks again
//!   when the queue runs short, as the native one searches again.
//! - The Foot's track or step logic still refuses a cell it cannot enter;
//!   a module route through blocked cells ends in the native refusal arms.
//!
//! ## `ApproachPoint`
//!
//! Where: the Foot body of Approach (`FootClass::Approach_Target @
//! 0x004D5690`, `movement::foot_approach`, `approach_foot_target`; Unit
//! `0x007414E0` and Infantry `0x00522340` call it from Attack, Hunt, Area
//! Guard, Rescue, Patrol and TechnoClass::AI's reach block), once the ring
//! search, a CloseRange= spot or a Jumpjet's first candidate chose the cell
//! (`0x004D689A`), before the destination is set (vt+0x480 with that cell).
//! The search runs in full first, so its scenario draws (a CloseRange=
//! infantryman's PlaceInfantryInCell spots) are unchanged (list 4.4). Not
//! asked when Approach ends before or without a chosen cell: the target in
//! range, a queued destination taken, the drop arms, a HunterSeeker's or
//! VehicleThief's target destination, the exhausted fallback.
//!
//! `subject` the Foot, `x`, `y`, `z` its position, `other` the target
//! object (0 for a cell target), `value2` the target's cell reference,
//! `cell` the chosen cell and `value` (and `native`) its reference. The
//! result is a cell reference: a cell on the map replaces the chosen cell;
//! 0, an object or a cell off the map is not an answer and keeps the
//! previous link's (the native cell for the first). The destination setter
//! then takes it as it takes the native one (`DestinationSet` included).

use vera20k_wasm_ext::decide::{EVENT_APPROACH_POINT, EVENT_PATH_FOUND, MAX_PATH_CELLS, PathCell};

use super::{ModuleEvent, Ref, cell_ref, decode_ref};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use crate::util::direction::{direction_delta, direction_from_delta};

impl Simulation {
    /// `PathFound` for Foot `id` searching toward `goal` (module docs):
    /// `native` the found route's directions, `None` for no route. `None`
    /// keeps the native result; `Some(None)` is no route; `Some(Some(_))`
    /// the replacing route's directions.
    pub(crate) fn module_path_found(
        &mut self,
        rules: &RuleSet,
        id: u64,
        goal: DriveCoord,
        native: Option<&[i32]>,
    ) -> Option<Option<Vec<i32>>> {
        if !self.trigger_runtime.module_decides(EVENT_PATH_FOUND) {
            return None;
        }
        let entity = self.substrate.entities.get(id)?;
        let start = (
            i32::from(entity.position.rx()),
            i32::from(entity.position.ry()),
        );
        let mut cells = Vec::new();
        let mut at = start;
        for &direction in native.unwrap_or(&[]).iter().take(MAX_PATH_CELLS) {
            // A tube step has no cell delta: not asked.
            let (dx, dy) = direction_delta(u8::try_from(direction).ok()?)?;
            at = (at.0 + dx, at.1 + dy);
            cells.push(PathCell { x: at.0, y: at.1 });
        }
        let goal_cell = crate::sim::movement::coord_cell(goal);
        let event = ModuleEvent {
            cell: u16::try_from(goal_cell.0)
                .ok()
                .zip(u16::try_from(goal_cell.1).ok()),
            value: cells.len() as i64,
            at_subject: true,
            ..ModuleEvent::new(EVENT_PATH_FOUND, id)
        };
        let mut area: Vec<u8> = cells.iter().flat_map(PathCell::to_bytes).collect();
        let answer = self.decide_module_area(rules, &event, cells.len() as i64, &mut area)?;
        let count = usize::try_from(answer).ok()?;
        if count == 0 {
            // An empty native route stays one.
            return match native {
                Some(directions) if directions.is_empty() => None,
                _ => Some(None),
            };
        }
        let answered = PathCell::read_all(&area, count)?;
        let terrain = self.resolved_terrain()?;
        let mut from = start;
        let mut directions = Vec::with_capacity(answered.len());
        for cell in answered {
            let on_map = u16::try_from(cell.x)
                .ok()
                .zip(u16::try_from(cell.y).ok())
                .is_some_and(|(x, y)| terrain.cell(x, y).is_some());
            if !on_map {
                return None;
            }
            let direction = direction_from_delta(cell.x - from.0, cell.y - from.1)?;
            directions.push(i32::from(direction));
            from = (cell.x, cell.y);
        }
        Some(Some(directions))
    }

    /// `ApproachPoint` for Foot `id` approaching `target`, whose search
    /// chose `native` (module docs): the cell it heads for.
    pub(crate) fn module_approach_point(
        &mut self,
        rules: &RuleSet,
        id: u64,
        target: TargetKind,
        native: (i16, i16),
    ) -> (i16, i16) {
        if !self.trigger_runtime.module_decides(EVENT_APPROACH_POINT) {
            return native;
        }
        let (Ok(x), Ok(y)) = (u16::try_from(native.0), u16::try_from(native.1)) else {
            return native;
        };
        let native_ref = cell_ref(x, y);
        let (other, target_cell) = super::control::target_fields(self, Some(target));
        let event = ModuleEvent {
            other,
            cell: Some((x, y)),
            value: native_ref,
            value2: target_cell.map_or(0, |(x, y)| cell_ref(x, y)),
            at_subject: true,
            ..ModuleEvent::new(EVENT_APPROACH_POINT, id)
        };
        let Some(answer) =
            self.decide_module_event(rules, &event, None, None, native_ref, only_cell)
        else {
            return native;
        };
        match decode_ref(answer) {
            Ok(Some(Ref::Cell { x, y })) if self.map_cell_in_bounds((x as i16, y as i16)) => {
                (x as i16, y as i16)
            }
            _ => native,
        }
    }
}

/// A cell reference; anything else is not an `ApproachPoint` answer.
fn only_cell(answer: i64) -> Option<i64> {
    matches!(decode_ref(answer), Ok(Some(Ref::Cell { .. }))).then_some(answer)
}
