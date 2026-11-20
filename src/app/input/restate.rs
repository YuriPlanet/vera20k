//! The mission restate briefing 0x10C (in-scenario state 9): its opening
//! read of MISSIONMD and its physical input.
//!
//! Entry: the sidebar Diplomacy gadget in campaign, and in LAN/WOL when the
//! mode is Cooperative (`0x006538FE`, input 0x80F2, [`super::diplomacy`]).
//! `ScenarioClass::ShowMissionRestateBriefing` 0x0065F520 re-reads the
//! MISSIONMD file the INI roots name on every open and resolves the text once
//! (`[0xB04CA0]`); a missing file opens nothing. The dialog sends no event:
//! in co-op the other peers are not told, and the local game keeps running
//! under it through the modal pump, as under Diplomacy.
//! Native zeroes `g_ScenarioStarted` while the dialog is open
//! (0x0065F537..0x0065F6C4) on the opening peer only. Its presentation reader,
//! the camera focus in `Tactical::AI` (0x006D256B), is held here through
//! [`crate::app::input::camera::advance_camera_ai`].
//! RESIDUAL: its gameplay readers (`FootClass::PointerExpired` 0x004D999B,
//! ParasiteClass, TagClass) and the session-query answer 0x005DFE7E would
//! diverge that peer, and VERA keeps the flag in the lockstep sim.

use crate::app::{App, AppState};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::page::PageEvent;
use crate::ui::shell::restate::{self, RestateControl, RestateLayout, restate_layout};
use winit::event::MouseButton;

/// Scenario+0x125C, the section MISSIONMD is read under: the loaded
/// scenario's file name, which a save carries. The section lookup is exact
/// case, so a Cooperative launch must name its map as MISSIONMD spells it
/// (`[C1A01MD.MAP]`); that spelling is unverified until Cooperative launches.
fn scenario_file_name(state: &AppState) -> Option<String> {
    let runtime = state.match_state.sim_runtime.runtime()?;
    Some(runtime.simulation.session().map_name().to_owned())
}

/// `0x0065F520`: read the briefing, then open 0x10C.
pub(crate) fn open(state: &mut AppState) {
    let Some(file_name) = scenario_file_name(state) else {
        log::info!("Mission briefing: no scenario file name to read MISSIONMD under");
        return;
    };
    let Some(assets) = state.process_assets.manager() else {
        return;
    };
    let Ok(selected) =
        crate::rules::retail_sources::select_ini(assets, assets.ini_roots().mission())
    else {
        // The CCINIClass finds no file: no dialog and no redraw.
        return;
    };
    let key = crate::rules::campaign_loading::restate_briefing_key(&selected.ini, &file_name);
    let text = crate::app::frontend::shell_pass::resolve_csf(state, &key).into_owned();
    state.match_state.match_presentation.restate_text = text;
    App::enter_in_game_menu_state(state, InGameMenuState::MissionBriefing);
}

pub(crate) fn layout(state: &AppState) -> Option<RestateLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(restate_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
    ))
}

/// One pointer message on 0x10C (`down`: `None` moves).
fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some(layout) = layout(state) else {
        return;
    };
    let event = super::dialog_page::shell_event(state, down);
    let ui = &mut state.match_state.match_presentation.dialog_ui;
    let events = super::dialog_page::route(
        ui,
        restate::DIALOG,
        restate::spec,
        |id| layout.rect(id),
        super::dialog_page::no_sync,
        event,
    );
    for event in super::dialog_page::play_cues(state, events) {
        if event == PageEvent::Activated(RestateControl::Return.id()) {
            // 0x0065F710 writes 0x423 to the result slot; State_Machine then
            // clears state 9 (0x0048C97A).
            App::enter_in_game_menu_state(state, InGameMenuState::Closed);
        }
    }
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    pointer(state, None);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button == MouseButton::Left {
        pointer(state, Some(pressed));
    }
}
