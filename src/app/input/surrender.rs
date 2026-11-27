//! The WOL tournament Surrender box: State_Machine state 2
//! (`0x0048CB3A..0x0048CC2A`, jump table `0x0048CC84` entry 2), which B5's
//! Abort requests in a tournament game (`0x004F16C0..0x004F16EA`).
//! `0x005C60D0` runs ShellMessageBox `0x005D3490` with `TXT_SURRENDER` and
//! `TXT_OK`/`TXT_CANCEL` on controls `0x5AE` and 2 (`0x005D35AE..0x005D35CA`);
//! only OK (result 0) surrenders. The box's callback `0x005D36A0` answers
//! controls 1 and 2 with result 1, so Cancel, Enter and Escape cancel.
//!
//! RESIDUAL: the box is drawn over the running battlefield; what native shows behind it after B5 closes is not
//! captured.

use crate::app::audio_runtime::AppAudioRuntime;
use crate::app::diagnostics::state::DiagnosticsState;
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::presentation::state::MatchPresentationState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::{App, PlatformState};
use crate::ui::game_screen::GameScreen;
use crate::ui::pause_menu::{InGameMenuState, ModalOutcome};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::{BoxParts, message_box_parts};
use crate::ui::shell::page::PageEvent;

const DIALOG: DialogResourceId = DialogResourceId(0x120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurrenderButton {
    /// `0x5AE`.
    Ok = 0x5AE,
    /// Control 2 (IDCANCEL).
    Cancel = 2,
}

/// State 2's arm. An observer (`[0xAC10C8]`) leaves at once through
/// Queue_Exit (`0x0048CB3A..0x0048CB47`); a House that is defeated, flagged
/// to die, has won or has lost (`+0x1F5..+0x1F8`) returns to state 0;
/// otherwise the box opens.
pub(crate) fn open(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    if frontend.multiplayer_shell_runtime.lan_observer() {
        App::apply_in_game_modal_outcome(
            platform,
            renderer,
            diag,
            frontend,
            match_state,
            process_assets,
            audio,
            persistence,
            ModalOutcome::LeaveMatch { destruct: false },
        );
        return;
    }
    let finished = match_state
        .sim_runtime
        .runtime()
        .and_then(|runtime| {
            let sim = &runtime.simulation;
            let house = sim.houses().get(&sim.session().current_house()?)?;
            Some(house.is_defeated() || house.is_to_die() || house.has_won() || house.has_lost())
        })
        .unwrap_or(true);
    App::enter_in_game_menu_state(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        if finished {
            InGameMenuState::Closed
        } else {
            InGameMenuState::Surrender
        },
    );
}

/// `0x120` centred over the frame the battlefield draws into.
pub(crate) fn layout(renderer: &RendererState, screen: &GameScreen) -> BoxParts {
    let (w, h) = (
        renderer.render_width(screen) as i32,
        renderer.render_height(screen) as i32,
    );
    message_box_parts(2, RectPx::new(0, 0, w, h))
}

/// The box's body static `0x5B0`, which takes no pointer.
pub(crate) const BODY: u16 = 0x5B0;

/// The box's widgets in the order they paint: the body, OK and Cancel.
fn nodes() -> vera20k_gui::NodeSpec {
    use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};
    let button = |id: SurrenderButton| NodeSpec::new(NodeKind::Button).id(id as i32);
    NodeSpec::new(NodeKind::Dialog)
        .child(
            NodeSpec::new(NodeKind::Text)
                .id(BODY.into())
                .with(Prop::Align, Value::Int(0)),
        )
        .child(button(SurrenderButton::Ok))
        .child(button(SurrenderButton::Cancel))
}

/// Where control `id` of the box stands.
fn rect_of(layout: &BoxParts, id: u16) -> Option<RectPx> {
    match id {
        BODY => Some(layout.body),
        0x5AE => Some(layout.button(0)),
        2 => Some(layout.button(1)),
        _ => None,
    }
}

/// The box's nodes on its layout before it paints: input syncs them only
/// on a pointer message.
pub(crate) fn sync(
    renderer: &RendererState,
    screen: &GameScreen,
    presentation: &mut MatchPresentationState,
) {
    if presentation.in_game_menu != InGameMenuState::Surrender {
        return;
    }
    let layout = layout(renderer, screen);
    let ui = &mut presentation.dialog_ui;
    let rect = |id| rect_of(&layout, id);
    super::dialog_page::sync_dialog(ui, DIALOG, nodes, rect, super::dialog_page::no_sync);
}

/// The box's widgets under its live dialog.
pub(crate) fn root(presentation: &MatchPresentationState) -> Option<vera20k_gui::NodeId> {
    presentation.dialog_ui.root_in(DIALOG)
}

/// One pointer message on the box (`down`: `None` moves).
pub(super) fn pointer(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    down: Option<bool>,
) {
    let layout = layout(renderer, &frontend.screen);
    let (x, y) = match_state.input.cursor_px();
    let event = super::dialog_page::event_at(x, y, down);
    let ui = &mut match_state.match_presentation.dialog_ui;
    let rect = |id| rect_of(&layout, id);
    let events =
        super::dialog_page::route(ui, DIALOG, nodes, rect, super::dialog_page::no_sync, event);
    for event in super::dialog_page::play_cues(
        frontend,
        match_state,
        process_assets,
        &mut audio.sfx_player,
        events,
    ) {
        match event {
            PageEvent::Activated(0x5AE) => activate(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                SurrenderButton::Ok,
            ),
            PageEvent::Activated(2) => activate(
                platform,
                renderer,
                diag,
                frontend,
                match_state,
                process_assets,
                audio,
                persistence,
                SurrenderButton::Cancel,
            ),
            _ => {}
        }
    }
    platform.window.request_redraw();
}

/// Whether `button` shows pushed.
pub(crate) fn pressed(presentation: &MatchPresentationState, button: SurrenderButton) -> bool {
    super::dialog_page::pressed(&presentation.dialog_ui, button as u16)
}

/// Enter (IDOK) and Escape (IDCANCEL) both give result 1.
pub(crate) fn key(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
) {
    activate(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        SurrenderButton::Cancel,
    );
}

/// OK queues EVENT_DESTRUCT for the player and Queue_Exit, clearing the
/// latch (`0x0048CBAE..0x0048CC23`); anything else returns to state 0.
fn activate(
    platform: &mut PlatformState,
    renderer: &RendererState,
    diag: &DiagnosticsState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    audio: &mut AppAudioRuntime,
    persistence: &mut PersistenceState,
    button: SurrenderButton,
) {
    App::apply_in_game_modal_outcome(
        platform,
        renderer,
        diag,
        frontend,
        match_state,
        process_assets,
        audio,
        persistence,
        match button {
            SurrenderButton::Ok => ModalOutcome::LeaveMatch { destruct: true },
            SurrenderButton::Cancel => ModalOutcome::Enter(InGameMenuState::Closed),
        },
    );
}
