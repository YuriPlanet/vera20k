//! VERA formation orders: the formation line drag, formation lock toggling and
//! the locked units' move cells.

use super::{context_order_receiver, finish_order, move_click_command, push_move_flash};
use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner;
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::map::entities::EntityCategory;
use crate::sim::combat::TargetKind;
use crate::sim::command::CommandEnvelope;
use crate::sim::intern::InternedId;

/// The selection's units that take a place in a VERA formation, in selection
/// order: the click's receivers but buildings, each with the cell from
/// `place`.
fn formation_units(
    sim: &crate::sim::world::Simulation,
    current_objects: &[u64],
    place: impl Fn(&crate::sim::game_entity::GameEntity) -> (i32, i32),
    ares: bool,
) -> Vec<(u64, (i32, i32))> {
    let control = sim.control_projection();
    sim.click_order_receivers(current_objects, ares)
        .into_iter()
        .filter(|&id| context_order_receiver(sim, &control, id))
        .filter_map(|id| {
            let entity = sim.entities().get(id)?;
            (entity.category() != EntityCategory::Structure).then(|| (id, place(entity)))
        })
        .collect()
}

/// How many of the selection's units a VERA formation line or block would
/// order: one dot each.
pub(crate) fn formation_unit_count(state: &AppState) -> usize {
    let Some(rt) = state.match_state.sim_runtime.runtime() else {
        return 0;
    };
    let current_objects = selected_stable_ids_in_order(
        Some(&rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    formation_units(
        &rt.simulation,
        &current_objects,
        standing_cell,
        rt.resources.rules.ares(),
    )
    .len()
}

/// The cell a unit stands on.
fn standing_cell(entity: &crate::sim::game_entity::GameEntity) -> (i32, i32) {
    (
        i32::from(entity.position.rx()),
        i32::from(entity.position.ry()),
    )
}

/// The cell a unit is heading for, its last queued destination first; the
/// cell it stands on when it has none or is heading for an object.
fn heading_cell(entity: &crate::sim::game_entity::GameEntity) -> (i32, i32) {
    let heading = entity
        .navigation
        .nav_queue()
        .last()
        .copied()
        .or(entity.navigation.nav_com());
    match heading {
        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => {
            (i32::from(rx), i32::from(ry))
        }
        _ => standing_cell(entity),
    }
}

/// VERA formation line (`input::formation_line`), not gamemd: the selection's
/// units each take their own point on the drawn line or in the dragged block;
/// [`crate::app::input::formation_line::assign`] decides who takes which.
///
/// Every order is the plain Cell click's own, as if that unit had been
/// clicked there alone: its destination from
/// [`crate::app::input::commands::ordinary_cell_move_goal`] and its Move, or
/// attack-move in that mode, from [`move_click_command`]. Buildings take no
/// point. The first selected object flashes and speaks, as for one click.
///
/// Returns `false`, having ordered nothing, in Planning Mode and when fewer
/// than two units can take a point; the release is then an ordinary click.
pub(crate) fn queue_formation_line(
    state: &mut AppState,
    path: &crate::app::input::formation_line::FormationPath,
) -> bool {
    let order_mode = state.match_state.input.queued_order_mode;
    let Some(owner) = preferred_local_owner(state) else {
        return false;
    };
    let current_objects = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .runtime()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    if state.match_state.input.planning.active() {
        return false;
    }
    let Some(rt) = state.match_state.sim_runtime.runtime() else {
        return false;
    };
    let resources = &rt.resources;
    let sim = &rt.simulation;
    let owner_id: InternedId = sim.interner().get(&owner).unwrap_or_default();
    let execute_tick = sim.session().tick();
    let control = sim.control_projection();
    let mut current_objects = current_objects;
    current_objects.retain(|&actor| context_order_receiver(sim, &control, actor));
    let units = formation_units(sim, &current_objects, standing_cell, resources.rules.ares());
    if units.len() < 2 {
        return false;
    }
    let cells = crate::app::match_runtime::sim_tick::tactical_cells(sim);
    let points: Vec<(i32, i32)> = path
        .points(units.len())
        .into_iter()
        .map(|(x, y)| {
            let cell = sim.clamped_click_cell(
                crate::app::match_runtime::sim_tick::world_point_to_cell(x, y, cells),
            );
            (i32::from(cell.0), i32::from(cell.1))
        })
        .collect();
    let mut queued: Vec<CommandEnvelope> = Vec::new();
    for (stable_id, cell) in crate::app::input::formation_line::assign(&units, &points) {
        let (Ok(rx), Ok(ry)) = (u16::try_from(cell.0), u16::try_from(cell.1)) else {
            continue;
        };
        push_move_flash(
            &mut queued,
            sim,
            &control,
            &resources.rules,
            owner_id,
            execute_tick,
            &current_objects,
            stable_id,
            (rx, ry),
            false,
        );
        let Some(goal) = crate::app::input::commands::ordinary_cell_move_goal_with(
            sim,
            &control,
            &resources.rules,
            owner_id,
            stable_id,
            (rx, ry),
            false,
        ) else {
            continue;
        };
        queued.push(CommandEnvelope::new(
            owner_id,
            execute_tick,
            move_click_command(
                sim,
                &resources.rules,
                order_mode,
                stable_id,
                TargetKind::Cell(goal.0, goal.1),
            ),
        ));
    }
    if queued.is_empty() {
        return false;
    }
    finish_order(state, queued, current_objects.first().copied(), false, None)
}

/// A locked unit's place as a map cell: one off the map's low edges becomes the
/// edge cell, for [`crate::sim::world::Simulation::clamped_click_cell`] to
/// bring into the playfield.
fn clamp_to_cell(cell: (i32, i32)) -> (u16, u16) {
    let edge = |value: i32| value.clamp(0, i32::from(i16::MAX)) as u16;
    (edge(cell.0), edge(cell.1))
}

/// VERA formation lock (`input::formation_lock`), not gamemd: lock the
/// selection's units into one formation, each at the cell it stands on or is
/// heading for; pressed on units that are one locked formation already, it
/// unlocks them. Says which happened in the message list. Fewer than two
/// units cannot be locked.
pub(crate) fn toggle_formation_lock(state: &mut AppState) {
    let current_objects = selected_stable_ids_in_order(
        state
            .match_state
            .sim_runtime
            .runtime()
            .map(|rt| &rt.simulation),
        state.rules(),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    );
    let Some(rt) = state.match_state.sim_runtime.runtime() else {
        return;
    };
    let sim = &rt.simulation;
    let units = formation_units(
        sim,
        &current_objects,
        heading_cell,
        rt.resources.rules.ares(),
    );
    let ids: Vec<u64> = units.iter().map(|&(id, _)| id).collect();
    let locks = &mut state.match_state.input.formation_locks;
    locks.retain(|id| sim.entities().get(id).is_some());
    let locked = if locks.is_one_formation(&ids) {
        locks.unlock(&ids);
        false
    } else if units.len() >= 2 {
        locks.lock(&units);
        true
    } else {
        return;
    };
    crate::app::input::messages::post_formation_lock(state, locked, units.len());
}

/// VERA formation lock: each receiver's own cell for a plain move click on
/// `click`, or `None` for an ordinary click
/// ([`crate::app::input::formation_lock::FormationLocks::move_cells`]).
pub(super) fn locked_move_cells(
    sim: &crate::sim::world::Simulation,
    locks: &crate::app::input::formation_lock::FormationLocks,
    receivers: &[u64],
    click: (u16, u16),
) -> Option<std::collections::HashMap<u64, (u16, u16)>> {
    let units: Vec<(u64, (i32, i32))> = receivers
        .iter()
        .map(|&id| Some((id, standing_cell(sim.entities().get(id)?))))
        .collect::<Option<_>>()?;
    let cells = locks.move_cells(&units, (i32::from(click.0), i32::from(click.1)))?;
    Some(
        cells
            .into_iter()
            .map(|(id, cell)| (id, sim.clamped_click_cell(clamp_to_cell(cell))))
            .collect(),
    )
}
