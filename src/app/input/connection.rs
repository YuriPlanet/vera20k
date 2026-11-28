//! Physical input of the connection dialog 0xEA (proc 0x0064AE50). While it
//! is open the wait loop services no tactical input (0x006497EC), so every
//! pointer event belongs to it.

use super::dialog_page::InGameDialog;
use crate::app::PlatformState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::audio::sfx::SfxPlayer;
use crate::ui::shell::connection::{self, ConnectionButton, ConnectionLayout, connection_layout};
use crate::ui::shell::page::PageEvent;
use winit::event::MouseButton;

/// 0xEA: no title, Quit Game as the lower right-panel button, on its own
/// controller.
pub(crate) const DIALOG: InGameDialog = InGameDialog {
    id: connection::DIALOG,
    title: None,
    lower: (0x2, connection::QUIT_LABEL),
    label: "Active Connection Shell EA",
    ui: |presentation| Some(&presentation.connection_ui),
    ui_mut: |presentation| Some(&mut presentation.connection_ui),
    own_checkbox_sound: false,
    help_under_held: false,
    help_fallback: |_| "",
};

/// Whether 0xEA is open; its controls start afresh on a new dialog.
fn open_controls(match_state: &mut MatchState) -> bool {
    let Some(page) = match_state.network_wait.page_mut() else {
        return false;
    };
    if !std::mem::replace(&mut page.controls_built, true) {
        match_state.match_presentation.connection_ui = Default::default();
    }
    true
}

/// Makes 0xEA's nodes current without input (rects, the bars, the list):
/// its renderer's call every drawn frame before it paints from them.
pub(crate) fn sync(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    persistence: &PersistenceState,
) -> Option<ConnectionLayout> {
    let layout = super::dialog_page::layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        connection_layout,
    )?;
    if !open_controls(match_state) {
        return None;
    }
    let match_state = match_state;
    let wait = &match_state.network_wait;
    let page = wait.page()?;
    let ui = &mut match_state.match_presentation.connection_ui;
    super::dialog_page::sync_dialog(
        ui,
        connection::DIALOG,
        connection::spec,
        |id| page.rect(&layout, id),
        |gui, root| connection::sync_nodes(gui, root, page, |source| wait.bar_timing(source)),
    );
    Some(layout)
}

/// One pointer message on 0xEA (`down`: `None` moves).
fn pointer(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    down: Option<bool>,
) {
    let Some(layout) = super::dialog_page::layout(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        connection_layout,
    ) else {
        return;
    };
    let event = super::dialog_page::shell_event(
        platform,
        renderer,
        &frontend.screen,
        match_state,
        persistence,
        down,
    );
    if !open_controls(match_state) {
        return;
    }
    let match_state = match_state;
    let wait = &match_state.network_wait;
    let Some(page) = wait.page() else {
        return;
    };
    let ui = &mut match_state.match_presentation.connection_ui;
    let events = super::dialog_page::route(
        ui,
        connection::DIALOG,
        connection::spec,
        |id| page.rect(&layout, id),
        |gui, root| connection::sync_nodes(gui, root, page, |source| wait.bar_timing(source)),
        event,
    );
    // Owner-draw buttons play GUIMainButtonSound at button-down
    // (0x0061374B..0x00613771).
    for event in
        super::dialog_page::play_cues(frontend, match_state, process_assets, sfx_player, events)
    {
        let PageEvent::Activated(id) = event else {
            continue;
        };
        let Some(button) = match_state
            .network_wait
            .page()
            .and_then(|page| page.button(id))
        else {
            continue;
        };
        match button {
            ConnectionButton::Quit => crate::app::match_runtime::network_match::request_leave(
                &mut match_state.network_wait,
            ),
            ConnectionButton::Player(row) => {
                crate::app::match_runtime::network_match::propose_kick(
                    platform,
                    renderer,
                    &frontend.screen,
                    match_state,
                    persistence,
                    process_assets,
                    row,
                )
            }
            ConnectionButton::PreviousPage | ConnectionButton::NextPage => {
                if let Some(page) = match_state.network_wait.page_mut()
                    && page.change_page(button == ConnectionButton::NextPage)
                {
                    let ui = &mut match_state.match_presentation.connection_ui;
                    ui.restart_instance(connection::DIALOG);
                }
                platform.window.request_redraw();
            }
        }
    }
}

pub(crate) fn cursor_moved(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
) {
    pointer(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        sfx_player,
        persistence,
        None,
    );
}

pub(crate) fn mouse(
    platform: &PlatformState,
    renderer: &RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    sfx_player: &mut Option<SfxPlayer>,
    persistence: &PersistenceState,
    button: MouseButton,
    pressed: bool,
) {
    if button == MouseButton::Left {
        pointer(
            platform,
            renderer,
            frontend,
            match_state,
            process_assets,
            sfx_player,
            persistence,
            Some(pressed),
        );
    }
}
