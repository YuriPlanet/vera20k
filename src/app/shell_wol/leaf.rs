//! What the WOL leaf pages (the persona questions, My Information, the
//! Buddy List and Quick Match) do with their `vera20k-gui`
//! widgets, done once: build them under the dialog, put them on the layout,
//! feed the pointer, play the controls' cues and read the hovered
//! control's status help. Each page keeps its node table, its rects and what
//! its control-id events do.

use vera20k_gui::{NodeId, NodeSpec};

use crate::app::frontend::skirmish_shell_render::gui_paint;
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::menu_page::MenuPageButtonRect;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// `dialog`'s root while it is the top dialog.
pub(crate) fn root(state: &AppState, dialog: DialogId) -> Option<NodeId> {
    let controller = &state.frontend.shell_controller;
    controller.root_in(dialog).filter(|_| controller.top_id() == Some(dialog))
}

/// Builds `dialog`'s widgets once and puts them on the layout; the root.
pub(super) fn sync(
    state: &mut AppState,
    dialog: DialogId,
    scale: ShellScale,
    spec: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
) -> Option<NodeId> {
    let root = root(state, dialog)?;
    let controller = &mut state.frontend.shell_controller;
    controller.set_scale(scale);
    page::ensure_built(controller.gui_mut(), root, spec);
    page::sync_rects(controller.gui_mut(), root, rect_of);
    Some(root)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Pointer {
    Move,
    Down,
    Up,
}

/// The pointer at the cursor into the top dialog's widgets: their cues play
/// (a list row's GenericClick is the page's to play) and the rest returns.
pub(super) fn pointer(state: &mut AppState, at: Pointer) -> Vec<PageEvent> {
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &mut state.frontend.shell_controller;
    match at {
        Pointer::Move => controller.on_pointer_move(x, y, &[]),
        Pointer::Down => controller.on_pointer_down(x, y, &[]),
        Pointer::Up => {
            controller.on_pointer_up(x, y, &[]);
        }
    }
    events(state)
}

fn events(state: &mut AppState) -> Vec<PageEvent> {
    let controller = &mut state.frontend.shell_controller;
    let raw = controller.drain_events();
    let events = page::page_events(controller.gui(), raw);
    let mut rest = Vec::with_capacity(events.len());
    for event in events {
        match event {
            PageEvent::Cue(cue) if cue != PageCue::ListClick => play(state, cue),
            event => rest.push(event),
        }
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
    rest
}

/// The open page's widget timers (a list's scroll-arrow repeat); the next
/// deadline.
pub(super) fn poll(state: &mut AppState, open: bool) -> Option<crate::util::time::Instant> {
    if !open {
        return None;
    }
    let controller = &mut state.frontend.shell_controller;
    let next = controller.poll();
    if !controller.drain_events().is_empty() {
        state.platform.window.request_redraw();
    }
    next
}

pub(super) fn play(state: &mut AppState, cue: PageCue) {
    let cue = match cue {
        PageCue::Button => return App::play_main_menu_button_sound(state),
        PageCue::Checkbox => LauncherCue::Checkbox,
        PageCue::ComboOpen => LauncherCue::ComboOpen,
        PageCue::ComboClose => LauncherCue::ComboClose,
        PageCue::ListClick => LauncherCue::GenericClick,
    };
    App::play_launcher_options_cue(state, cue);
}

/// The hovered control's status help on `dialog`, under a box too.
pub(crate) fn status_key(state: &AppState, dialog: DialogId) -> Option<&str> {
    let controller = &state.frontend.shell_controller;
    page::hovered_key_in(controller.gui(), controller.root_in(dialog)?)
}

/// `dialog`'s right-panel buttons as the column paints them.
pub(crate) fn buttons(
    state: &AppState,
    dialog: DialogId,
    buttons: &[MenuPageButtonRect],
) -> Vec<(MenuPageButtonRect, bool, bool)> {
    let controller = &state.frontend.shell_controller;
    page::menu_button_looks(controller.gui(), controller.root_in(dialog), buttons)
}

/// The widgets under `dialog`'s root through the generic painter.
pub(crate) fn paint(
    state: &AppState,
    dialog: DialogId,
    scale: ShellScale,
) -> gui_paint::GuiPaint {
    let controller = &state.frontend.shell_controller;
    match controller.root_in(dialog) {
        Some(root) => gui_paint::paint_gui_children(
            state,
            controller.gui(),
            root,
            scale,
        ),
        None => Default::default(),
    }
}
