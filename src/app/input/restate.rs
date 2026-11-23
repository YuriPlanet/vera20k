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

use super::dialog_page::InGameDialog;
use crate::app::{App, AppState};
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::page::PageEvent;
use crate::ui::shell::restate::{self, RestateControl};

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
    let text = state.csf_text(&key);
    state.match_state.match_presentation.restate_text = text;
    App::enter_in_game_menu_state(state, InGameMenuState::MissionBriefing);
}

/// 0x10C: title 0x694 and Return 0x423 in Diplomacy's right-panel places.
pub(crate) const DIALOG: InGameDialog = InGameDialog {
    id: restate::DIALOG,
    title: Some(restate::TITLE_LABEL),
    lower: (0x423, restate::RETURN_LABEL),
    label: "Active Restate Shell 10C",
    ui: super::dialog_page::shared_ui,
    ui_mut: super::dialog_page::shared_ui_mut,
    own_checkbox_sound: false,
};

/// One pointer message on 0x10C (`down`: `None` moves).
pub(super) fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some(layout) = super::dialog_page::layout(state, restate::restate_layout) else {
        return;
    };
    let rect = |id| layout.rect(id);
    let events = DIALOG.route(
        state,
        down,
        restate::spec,
        rect,
        super::dialog_page::no_sync,
    );
    if events.contains(&PageEvent::Activated(RestateControl::Return.id())) {
        // 0x0065F710 writes 0x423 to the result slot; State_Machine then
        // clears state 9 (0x0048C97A).
        App::enter_in_game_menu_state(state, InGameMenuState::Closed);
    }
}
