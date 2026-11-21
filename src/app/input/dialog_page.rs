//! The in-game dialogs' controls on `vera20k-gui` ([`crate::ui::shell::page`]):
//! each dialog describes its controls once (resource id and status help in
//! every node), keeps them on its layout and reads what the pointer did as
//! the shell's cues and control-id events. The scenario's modal dialogs share
//! [`crate::app::presentation::state::MatchPresentationState::dialog_ui`];
//! a dialog with state of its own (B8, A3, 0xEA) owns its controller.

use vera20k_gui::{Gui, InputEvent, MouseButton, NodeId, NodeKind, NodeSpec};

use crate::app::{App, AppState};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::page::{self, PageEvent};

/// A left-button message at `(x, y)`: `down`, or a move for `None`.
pub(crate) fn event_at(x: i32, y: i32, down: Option<bool>) -> InputEvent {
    let button = MouseButton::Left;
    match down {
        None => InputEvent::PointerMove { x, y },
        Some(true) => InputEvent::PointerDown { x, y, button },
        Some(false) => InputEvent::PointerUp { x, y, button },
    }
}

/// [`event_at`] the active-game shell's cursor.
pub(crate) fn shell_event(state: &AppState, down: Option<bool>) -> InputEvent {
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    event_at(x, y, down)
}

/// Builds `dialog`'s controls on `ui` once, keeps them on `rect_of` (a
/// control without a rect is hidden), lets `sync` put the dialog's values on
/// them, routes `event` and returns its control events, cues included, in
/// order.
pub(crate) fn route(
    ui: &mut DialogController,
    dialog: DialogResourceId,
    nodes: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
    sync: impl FnOnce(&mut Gui, NodeId),
    event: InputEvent,
) -> Vec<PageEvent> {
    ui.ensure_active(dialog);
    crate::ui::shell::menu_page::sync_nodes(ui, dialog, nodes, rect_of, &[]);
    if let Some(root) = ui.root_in(dialog) {
        sync(ui.gui_mut(), root);
    }
    ui.on_input(event);
    let events = ui.drain_events();
    page::page_events(ui.gui(), events)
}

/// [`route`] with nothing to sync.
pub(crate) fn no_sync(_: &mut Gui, _: NodeId) {}

/// An in-game shell dialog over the active-game shell (`0x00621E90` ->
/// `0x0072F540`), as its router and painter
/// ([`crate::app::frontend::skirmish_shell_render`]) share it. Every one has
/// the right-panel title static 0x694, a lower right-panel button
/// (`0x0060B000`) and the status line 0x695 in the same places; its own
/// controls are a [`NodeSpec`] kept on its layout's rects.
pub(crate) struct InGameDialog {
    pub(crate) id: DialogResourceId,
    /// Title 0x694's caption, when it shows one.
    pub(crate) title: Option<(&'static str, &'static str)>,
    /// The lower right-panel button's id and caption.
    pub(crate) lower: (u16, (&'static str, &'static str)),
    /// The render pass's label.
    pub(crate) label: &'static str,
    /// The controller its controls live on.
    pub(crate) ui: fn(&AppState) -> Option<&DialogController>,
    pub(crate) ui_mut: fn(&mut AppState) -> Option<&mut DialogController>,
    /// Its boxes play GUICheckboxSound where their proc cases do, not at
    /// the toggle.
    pub(crate) own_checkbox_sound: bool,
}

impl InGameDialog {
    /// One pointer message at the shell cursor (`down`: `None` moves),
    /// [`route`]d on the dialog's controller, its cues played; returns the
    /// rest.
    pub(crate) fn route(
        &self,
        state: &mut AppState,
        down: Option<bool>,
        nodes: impl FnOnce() -> NodeSpec,
        rect_of: impl Fn(u16) -> Option<RectPx>,
        sync: impl FnOnce(&mut Gui, NodeId),
    ) -> Vec<PageEvent> {
        let event = shell_event(state, down);
        let Some(ui) = (self.ui_mut)(state) else {
            return Vec::new();
        };
        let mut events = route(ui, self.id, nodes, rect_of, sync, event);
        if self.own_checkbox_sound {
            events.retain(|event| *event != PageEvent::Cue(page::PageCue::Checkbox));
        }
        play_cues(state, events)
    }
}

/// The controller the scenario's modal dialogs share.
pub(crate) fn shared_ui(state: &AppState) -> Option<&DialogController> {
    Some(&state.match_state.match_presentation.dialog_ui)
}

pub(crate) fn shared_ui_mut(state: &mut AppState) -> Option<&mut DialogController> {
    Some(&mut state.match_state.match_presentation.dialog_ui)
}

/// A dialog's `layout` on the active-game shell as it stands.
pub(crate) fn layout<L>(
    state: &AppState,
    layout: impl FnOnce(i32, i32, crate::ui::shell::in_game_shell::InGameShellLayout, [i32; 2]) -> L,
) -> Option<L> {
    Some(
        crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?
            .place(layout),
    )
}

/// One left-button message (`down`: `None` moves) on the in-game shell
/// dialog `menu` shows, if it is one of these.
pub(crate) fn in_game_pointer(
    state: &mut AppState,
    menu: crate::ui::pause_menu::InGameMenuState,
    down: Option<bool>,
) {
    use crate::ui::pause_menu::InGameMenuState;
    match menu {
        InGameMenuState::Menu => super::pause_menu::pointer(state, down),
        InGameMenuState::AbortConfirm => super::abort::pointer(state, down),
        InGameMenuState::Surrender => super::surrender::pointer(state, down),
        InGameMenuState::Sound => super::sound::pointer(state, down),
        InGameMenuState::Diplomacy => super::diplomacy::pointer(state, down),
        InGameMenuState::MissionBriefing => super::restate::pointer(state, down),
        _ => {}
    }
}

/// Plays the cues among `events` (an enabled button's GUIMainButtonSound at
/// mouse down, `0x0061374B`) and returns the rest.
pub(crate) fn play_cues(state: &mut AppState, events: Vec<PageEvent>) -> Vec<PageEvent> {
    let mut rest = Vec::with_capacity(events.len());
    for event in events {
        match event {
            PageEvent::Cue(cue) => App::play_page_cue(state, cue),
            event => rest.push(event),
        }
    }
    rest
}

/// Whether control `id` shows pushed: captured and still under the pointer
/// (612B70).
pub(crate) fn pressed(ui: &DialogController, id: u16) -> bool {
    let gui = ui.gui();
    let Some(root) = ui.top_id().and_then(|dialog| ui.root_in(dialog)) else {
        return false;
    };
    gui.find_id(root, id.into())
        .is_some_and(|node| gui.shows_pressed(node))
}

/// The control under the pointer.
pub(crate) fn hovered(ui: &DialogController) -> Option<u16> {
    u16::try_from(ui.gui().id_of(ui.gui().hovered()?)?).ok()
}

/// The trackbar holding the pointer: the GUI hovers nothing until its
/// release (`SetCapture` 0x0061E4DF).
pub(crate) fn held_trackbar(ui: &DialogController) -> Option<u16> {
    let gui = ui.gui();
    let node = gui
        .pressed()
        .filter(|node| gui.tree().kind(*node) == Ok(NodeKind::Trackbar))?;
    u16::try_from(gui.id_of(node)?).ok()
}

/// The control a press at `(x, y)` lands on in `ui`'s top dialog.
pub(crate) fn hit(ui: &DialogController, x: i32, y: i32) -> Option<u16> {
    let root = ui.root_in(ui.top_id()?)?;
    let gui = ui.gui();
    u16::try_from(gui.id_of(gui.hit_at(root, x, y, true)?)?).ok()
}

/// The status help of the control under the pointer (`0x006040B0`).
pub(crate) fn help(ui: &DialogController) -> Option<&str> {
    page::hovered_key(ui.gui()).filter(|key| !key.is_empty())
}
