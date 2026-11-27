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
use crate::app::App;
use crate::app::PlatformState;
use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::sim_host::SimHost;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::ui::pause_menu::InGameMenuState;
use crate::ui::shell::page::PageEvent;
use crate::ui::shell::restate::{self, RestateControl};

/// Scenario+0x125C, the section MISSIONMD is read under: the loaded
/// scenario's file name, which a save carries. The section lookup is exact
/// case, so a Cooperative launch must name its map as MISSIONMD spells it
/// (`[C1A01MD.MAP]`); that spelling is unverified until Cooperative launches.
fn scenario_file_name(sim_runtime: &SimHost) -> Option<String> {
    let runtime = sim_runtime.runtime()?;
    Some(runtime.simulation.session().map_name().to_owned())
}

/// `0x0065F520`: read the briefing, then open 0x10C.
pub(crate) fn open(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    let Some(file_name) = scenario_file_name(&match_state.sim_runtime) else {
        log::info!("Mission briefing: no scenario file name to read MISSIONMD under");
        return;
    };
    let Some(assets) = process_assets.manager() else {
        return;
    };
    let Ok(selected) =
        crate::rules::retail_sources::select_ini(assets, assets.ini_roots().mission())
    else {
        // The CCINIClass finds no file: no dialog and no redraw.
        return;
    };
    let key = crate::rules::campaign_loading::restate_briefing_key(&selected.ini, &file_name);
    let text = process_assets.csf_text(&key);
    match_state.match_presentation.restate_text = text;
    App::enter_in_game_menu_state(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        InGameMenuState::MissionBriefing,
    );
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
    help_under_held: false,
    help_fallback: |_| "",
};

/// Makes 0x10C's nodes current without input (rects): its renderer's call
/// every drawn frame before it paints from them.
pub(crate) fn sync(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
) -> Option<restate::RestateLayout> {
    let layout = super::dialog_page::layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        restate::restate_layout,
    )?;
    let rect = |id| layout.rect(id);
    // The briefing 0x6E7 into its node, copied only when it changes; the
    // text steps out of the state while the dialog's controller is borrowed.
    let presentation = &mut match_state.match_presentation;
    let briefing = std::mem::take(&mut presentation.restate_text);
    DIALOG.sync(
        &mut match_state.match_presentation,
        restate::spec,
        rect,
        |gui, root| {
            let content = RestateControl::Content.id();
            crate::ui::shell::page::set_text(
                gui,
                root,
                content,
                vera20k_gui::Prop::Text,
                &briefing,
            );
        },
    );
    match_state.match_presentation.restate_text = briefing;
    Some(layout)
}

/// One pointer message on 0x10C (`down`: `None` moves).
pub(super) fn pointer(
    platform: &mut PlatformState,
    renderer: &RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    down: Option<bool>,
) {
    let Some(layout) = super::dialog_page::layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        restate::restate_layout,
    ) else {
        return;
    };
    let rect = |id| layout.rect(id);
    let events = DIALOG.route(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        persistence,
        down,
        restate::spec,
        rect,
        super::dialog_page::no_sync,
    );
    if events.contains(&PageEvent::Activated(RestateControl::Return.id())) {
        // 0x0065F710 writes 0x423 to the result slot; State_Machine then
        // clears state 9 (0x0048C97A).
        App::enter_in_game_menu_state(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            InGameMenuState::Closed,
        );
    }
}
