//! Power mode (Display `+0x11B2`): the cursor that switches the player's own
//! buildings off and on. YR never arms it; Ares 3.0's TogglePower command
//! does. The mode byte is the sidebar gadget state's
//! ([`crate::ui::sidebar::gadget_flash::SidebarGadgetState::set_power_mode`]);
//! the switch itself is the existing On/Off Line event
//! ([`Command::SetBuildingOnLine`]).

use crate::app::PlatformState;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::world::Simulation;

/// Ares 3.0 `TogglePowerCommandClass::Execute` (Ares.dll `100025C0`,
/// registered by CommandClassCallback_Register, hook `0x533058`,
/// `10002A80`): with RulesExt `TogglePowerAllowed` (`+0x1C`) set it calls
/// `SetTogglePowerMode(-1)`; entry unselects (`0x004AC89B`). The command
/// exists only while Ares plays.
pub(crate) fn execute_command(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    if !crate::app::state::active_rules(frontend, match_state)
        .is_some_and(|rules| rules.ares() && rules.ares_toggle_power_allowed())
    {
        return;
    }
    let has_building = match_state
        .sim_runtime
        .runtime()
        .is_some_and(|runtime| super::dispatch::local_has_manageable_building(&runtime.simulation));
    let placement_armed = match_state.input.armed_building_type().is_some();
    if match_state
        .match_presentation
        .sidebar_gadget_state
        .set_power_mode(true, placement_armed, i32::from(has_building))
    {
        super::dispatch::unselect_all(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
        );
    }
}

/// `SetTogglePowerMode(0)`, which Cancel (`0x004AADDE`), the team keys
/// (`0x007311D3`, `0x007313AC`, `0x007314CB`) and Next/Previous Object
/// (`0x00536617`, `0x00536A87`) call before their other mode clears.
pub(crate) fn disable(match_state: &mut MatchState) {
    let placement_armed = match_state.input.armed_building_type().is_some();
    match_state
        .match_presentation
        .sidebar_gadget_state
        .set_power_mode(false, placement_armed, 0);
}

/// DetermineAction's power arm for the object under the cursor: whether it
/// answers TogglePower (0x21) rather than NoTogglePower (0x22). The stock arm
/// (`0x00692815..0x00692893`) takes a Building (vt+0x2C) of the local player
/// (vt+0x3C, IsHumanPlayer `0x0050B6F0`) that may be selected (vt+0x138),
/// is no 1x1 undeployer (vt+0x80), and whose type has `TogglePower=` and a
/// positive drain or `Powered=` (YRpp `CanTogglePower`). Ares 3.0
/// DisplayClass_ChooseAction_TogglePower (`1005C790`, hook `0x0069281E`,
/// resuming at `0x0069289B`) also refuses one warping out (vt+0x1D4) or
/// under EMP (vt+0x37C). The owner test is the local player's, as for the
/// Repair and Sell arms.
pub(crate) fn admits(sim: &Simulation, rules: &RuleSet, stable_id: u64) -> bool {
    let control = sim.control_projection();
    let (Some(player), Some(building)) =
        (control.local_command_house(), sim.entities().get(stable_id))
    else {
        return false;
    };
    let Some(object) = sim.interner().object_type(building.type_ref(), rules) else {
        return false;
    };
    building.category() == EntityCategory::Structure
        && control.can_manage_from(building.owner(), player)
        && sim.object_is_selectable(stable_id, rules)
        && !object.is_1x1_with_undeploy()
        && !(rules.ares() && (building.is_warped_out() || building.is_under_emp()))
        && object.can_toggle_power()
}

/// The building under the cursor that [`admits`] power mode's switch.
pub(crate) fn building_under_cursor(
    platform: &PlatformState,
    frontend: &FrontendState,
    match_state: &MatchState,
) -> Option<u64> {
    let (world_x, world_y) = crate::app::match_runtime::sim_tick::screen_point_to_world(
        &match_state.input,
        match_state.input.cursor_x,
        match_state.input.cursor_y,
    );
    let id = super::commands::visible_object_under_point(
        platform,
        frontend,
        match_state,
        world_x,
        world_y,
    )?;
    let runtime = match_state.sim_runtime.runtime()?;
    admits(
        &runtime.simulation,
        crate::app::state::active_rules(frontend, match_state)?,
        id,
    )
    .then_some(id)
}

/// `DisplayClass::BandBox_LeftUp @ 0x004AB9B0`'s TogglePower arm
/// (`0x004ABFB3..0x004AC006`): over a Building, an online one (`+0x660`)
/// queues the Off Line event (2), any other the On Line event (1), as the
/// player's House. NoTogglePower does nothing. With nothing selected the
/// selection dispatch before it (`0x004AE750`) has no receiver.
pub(crate) fn click(
    platform: &PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    let Some(entity_id) = building_under_cursor(platform, frontend, match_state) else {
        return;
    };
    let Some(online) = match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| runtime.simulation.entities().get(entity_id))
        .map(crate::sim::game_entity::GameEntity::building_online)
    else {
        return;
    };
    let Some(owner) = super::commands::preferred_command_owner(&match_state.sim_runtime) else {
        return;
    };
    super::commands::schedule_command(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        &owner,
        Command::SetBuildingOnLine {
            entity_id,
            on: !online,
        },
    );
}

#[cfg(all(test, tests_app_input_d_z))]
#[path = "power_mode_tests.rs"]
mod tests;
