//! What the WOL leaf pages (the persona questions, My Information, the
//! Buddy List and Quick Match) do with their `vera20k-gui`
//! widgets, done once: build them under the dialog, put them on the layout,
//! feed the pointer, play the controls' cues and read the hovered
//! control's status help. Each page keeps its node table, its rects and what
//! its control-id events do.

use vera20k_gui::{NodeId, NodeSpec, Prop, Value};

use crate::app::frontend::skirmish_shell_render::gui_paint;
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::menu_page::MenuPageButtonRect;
use crate::ui::shell::page::{self, PageCue, PageEvent};

/// `dialog`'s root while it takes the input: the screen's top dialog, or the top overlay over it.
pub(crate) fn root(state: &AppState, dialog: DialogId) -> Option<NodeId> {
    let controller = &state.frontend.shell_controller;
    let takes = |top: Option<DialogId>| top == Some(dialog);
    controller
        .root_in(dialog)
        .filter(|_| takes(controller.top_id()) || takes(controller.overlay_id()))
}

/// Builds `dialog`'s widgets once and puts them on the layout; the root.
pub(super) fn sync(
    state: &mut AppState,
    dialog: DialogId,
    scale: ShellScale,
    spec: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
) -> Option<NodeId> {
    let root = place(state, dialog, scale, spec, rect_of)?;
    // Only the focused edit takes keys, so only it scrolls; an assigned text
    // shows from its start.
    if let Some(node) = state.frontend.shell_controller.gui().focus() {
        scroll_edit(state, node, scale);
    }
    Some(root)
}

/// [`sync`] without scrolling the edits, for a screen that scrolls them
/// itself ([`scroll_edit`]).
pub(super) fn place(
    state: &mut AppState,
    dialog: DialogId,
    scale: ShellScale,
    spec: impl FnOnce() -> NodeSpec,
    rect_of: impl Fn(u16) -> Option<RectPx>,
) -> Option<NodeId> {
    let root = root(state, dialog)?;
    let controller = &mut state.frontend.shell_controller;
    controller.set_scale_in(dialog, scale);
    page::ensure_built(controller.gui_mut(), root, spec);
    page::sync_rects(controller.gui_mut(), root, rect_of);
    Some(root)
}

/// NewEdit keeps its caret in view (`scroll_to_caret`, text widths in the
/// box's authored pixels); the first shown unit lives in the edit's `Top`.
/// Anything but an edit is left alone.
pub(super) fn scroll_edit(state: &mut AppState, node: NodeId, scale: ShellScale) {
    let font = &state.renderer.bit_font;
    let gui = state.frontend.shell_controller.gui_mut();
    if gui.tree().kind(node) != Ok(vera20k_gui::NodeKind::Edit) {
        return;
    }
    let Ok(rect) = gui.tree().screen_rect(node) else {
        return;
    };
    let mut edit = gui_paint::new_edit(gui, node);
    edit.scroll_to_caret((scale.base_rect(rect).w - 3).max(0) as u32, |text| {
        font.text_width(text)
    });
    let _ = gui.set(node, Prop::Top, Value::Int(edit.first_visible_unit as i32));
}

/// Control `id`'s text under `dialog`'s root ("" when it has none).
pub(crate) fn text(state: &AppState, dialog: DialogId, id: u16) -> String {
    state.frontend.shell_controller.text(dialog, id)
}

/// Whether control `id` under `dialog`'s root is there and enabled.
pub(crate) fn enabled(state: &AppState, dialog: DialogId, id: u16) -> bool {
    let controller = &state.frontend.shell_controller;
    controller.control(dialog, id).is_some() && controller.flag(dialog, id, Prop::Enabled, true)
}

/// The edit holding the top dialog's keyboard focus, by control id.
pub(crate) fn focused(state: &AppState) -> Option<u16> {
    let gui = state.frontend.shell_controller.gui();
    gui.id_of(gui.focus()?)
        .and_then(|id| u16::try_from(id).ok())
}

/// `SetFocus` on edit `id` under `dialog`'s root (`None` clears the focus).
pub(super) fn focus(state: &mut AppState, dialog: DialogId, id: Option<u16>) {
    state.frontend.shell_controller.focus_control(dialog, id);
}

/// A key or typed text as the widgets take it.
pub(super) fn input_of(
    code: Option<winit::keyboard::KeyCode>,
    text: Option<&str>,
) -> Option<vera20k_gui::InputEvent> {
    use vera20k_gui::{InputEvent, Key};
    use winit::keyboard::KeyCode;
    Some(match code {
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => InputEvent::Key(Key::Enter),
        Some(KeyCode::Escape) => InputEvent::Key(Key::Escape),
        Some(KeyCode::Tab) => InputEvent::Key(Key::Tab),
        Some(KeyCode::Backspace) => InputEvent::Key(Key::Backspace),
        Some(KeyCode::Delete) => InputEvent::Key(Key::Delete),
        Some(KeyCode::ArrowLeft) => InputEvent::Key(Key::Left),
        Some(KeyCode::ArrowRight) => InputEvent::Key(Key::Right),
        Some(KeyCode::ArrowUp) => InputEvent::Key(Key::Up),
        Some(KeyCode::ArrowDown) => InputEvent::Key(Key::Down),
        Some(KeyCode::Home) => InputEvent::Key(Key::Home),
        Some(KeyCode::End) => InputEvent::Key(Key::End),
        Some(KeyCode::PageUp) => InputEvent::Key(Key::PageUp),
        Some(KeyCode::PageDown) => InputEvent::Key(Key::PageDown),
        _ => match text {
            Some(text) if !text.is_empty() => InputEvent::Text(text.to_owned()),
            _ => return None,
        },
    })
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
    let events = pointer_quiet(state, at);
    state.frontend.shell_status_line.repaint();
    events
}

/// [`pointer`] without repainting the status line, for a dialog whose hit
/// test writes its status help only on a move (a hover message, `0x4B2`).
pub(super) fn pointer_quiet(state: &mut AppState, at: Pointer) -> Vec<PageEvent> {
    let (x, y) = state.match_state.input.cursor_px();
    let controller = &mut state.frontend.shell_controller;
    match at {
        Pointer::Move => controller.on_pointer_move(x, y, &[]),
        Pointer::Down => controller.on_pointer_down(x, y, &[]),
        Pointer::Up => {
            controller.on_pointer_up(x, y, &[]);
        }
    }
    let events = events(state);
    state.platform.window.request_redraw();
    events
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
pub(crate) fn paint(state: &AppState, dialog: DialogId, scale: ShellScale) -> gui_paint::GuiPaint {
    let controller = &state.frontend.shell_controller;
    match controller.root_in(dialog) {
        Some(root) => gui_paint::paint_gui_children(state, controller.gui(), root, scale),
        None => Default::default(),
    }
}
