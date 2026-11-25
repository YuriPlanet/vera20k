//! Display power mode (`+0x11B2`) switches buildings off and on through
//! the existing On/Off Line event, `Command::SetBuildingOnLine`.

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::Simulation;

/// Ares 3.0p1 TogglePowerCommandClass::Execute (`0x10002580`, registered
/// at `0x533058 -> 0x10002A40`) calls SetTogglePowerMode(-1) when
/// RulesExt+0x1C is set. Entering power mode unselects (`0x004AC89B`).
pub(crate) fn execute_command(state: &mut AppState) {
    if !state.rules().is_some_and(RuleSet::toggle_power_allowed) {
        return;
    }
    let has_building = state
        .match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| super::dispatch::local_has_manageable_building(&runtime.simulation));
    let placement_armed = state.armed_building_type().is_some();
    if state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .set_power_mode(true, placement_armed, i32::from(has_building))
    {
        super::dispatch::unselect_all(state);
    }
}

/// SetTogglePowerMode(0) from Cancel (`0x004AADDE`), team recall keys
/// (`0x007311D3`, `0x007313AC`, `0x007314CB`) and Next/Previous Object
/// (`0x00536617`, `0x00536A87`).
pub(crate) fn disable(state: &mut AppState) {
    let placement_armed = state.armed_building_type().is_some();
    state
        .match_state
        .match_presentation
        .sidebar_gadget_state
        .set_power_mode(false, placement_armed, 0);
}

/// Ares 3.0p1 ChooseAction_TogglePower (`0x0069281E -> 0x1005D4B0`)
/// replaces the stock choice through `0x0069289B`. It chooses action 0x21
/// for a selectable building of a human or PlayerControl team (local
/// PlayerPtr in multiplayer), excluding 1x1 undeployers, warped buildings
/// and EMP victims. Other targets get 0x22.
fn admits(sim: &Simulation, rules: &RuleSet, stable_id: u64) -> bool {
    let control = sim.control_projection();
    let Some(building) = sim.entities().get(stable_id) else {
        return false;
    };
    let Some(object) = sim.interner().object_type(building.type_ref(), rules) else {
        return false;
    };
    rules.toggle_power_allowed()
        && building.category() == EntityCategory::Structure
        && control.local_command_house().is_some()
        && control.can_manage(building.owner())
        && sim.object_is_selectable(stable_id, rules)
        && !object.is_1x1_with_undeploy()
        && !building.is_warped_out()
        && !building.is_under_emp()
        && object.can_toggle_power()
}

pub(crate) fn building_under_cursor(state: &AppState) -> Option<u64> {
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let id = super::commands::visible_object_under_point(state, world_x, world_y)?;
    let runtime = state.match_state.sim_runtime.runtime()?;
    admits(&runtime.simulation, state.rules()?, id).then_some(id)
}

/// Ares 3.0p1 LeftMouseButtonUp_ExecPowerToggle
/// (`0x004ABFBE -> 0x10058B70`) rechecks the building and its human
/// owner, then resumes at `0x004ABFCE`. The Building+0x660 latch
/// determines which EventClass record to queue, Off Line (2) or On Line (1).
/// Both use PlayerPtr's House index (`0x004ABFF3`, `0x004AC020`).
pub(crate) fn click(state: &mut AppState) {
    let Some(entity_id) = building_under_cursor(state) else {
        return;
    };
    let Some(online) = state
        .match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| runtime.simulation.entities().get(entity_id))
        .map(crate::sim::game_entity::GameEntity::building_online)
    else {
        return;
    };
    let Some(owner) = super::commands::preferred_command_owner(state) else {
        return;
    };
    super::commands::schedule_command(
        state,
        &owner,
        Command::SetBuildingOnLine {
            entity_id,
            on: !online,
        },
    );
}
