//! Shell dialog stack and pointer input, run on the `vera20k-gui` tree.
//!
//! Each pushed dialog is a `Dialog` node; a screen builds its controls under
//! it as nodes keyed by resource id, so hit-testing, capture and hover are
//! the GUI crate's (612B70: release activates only on the captured, enabled
//! control; a disabled control still hovers, so it keeps driving its
//! tooltip).

use vera20k_gui::{Gui, InputEvent, Key, MouseButton, NodeId, NodeKind, Prop, UiEvent, Value};
use winit::keyboard::KeyCode;

use super::descriptor::DialogResourceId;
use crate::util::time::{Duration, Instant};

/// The shell's one keyboard adapter: the GUI key a physical key gives
/// (`shift` turns Tab into Shift+Tab), none for a key the widgets do not
/// read.
pub fn gui_key(code: KeyCode, shift: bool) -> Option<Key> {
    Some(match code {
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Tab if shift => Key::BackTab,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    })
}

/// One input per `WM_CHAR`: typed text a character at a time, so an edit's
/// parent runs its `EN_CHANGE` handler after each.
pub fn per_char(event: InputEvent) -> Vec<InputEvent> {
    match event {
        InputEvent::Text(text) => text.chars().map(|c| InputEvent::Text(c.into())).collect(),
        event => vec![event],
    }
}

/// [`gui_input`] as [`per_char`] inputs.
pub fn gui_inputs(code: Option<KeyCode>, shift: bool, text: Option<&str>) -> Vec<InputEvent> {
    gui_input(code, shift, text).map_or_else(Vec::new, per_char)
}

/// [`gui_key`], else the text the key typed, as one GUI input.
pub fn gui_input(code: Option<KeyCode>, shift: bool, text: Option<&str>) -> Option<InputEvent> {
    match (code.and_then(|code| gui_key(code, shift)), text) {
        (Some(key), _) => Some(InputEvent::Key(key)),
        (None, Some(text)) if !text.is_empty() => Some(InputEvent::Text(text.to_owned())),
        _ => None,
    }
}

/// Native list notifications use the host's double-click policy.
#[cfg(windows)]
pub fn host_double_click_limits() -> (Duration, i32, i32) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDoubleClickTime() -> u32;
        fn GetSystemMetrics(index: i32) -> i32;
    }
    // SAFETY: pointer-free Win32 queries with no ownership effects.
    unsafe {
        (
            Duration::from_millis(u64::from(GetDoubleClickTime())),
            GetSystemMetrics(36),
            GetSystemMetrics(37),
        )
    }
}
#[cfg(not(windows))]
pub fn host_double_click_limits() -> (Duration, i32, i32) {
    (Duration::from_millis(500), 4, 4)
}

struct Instance {
    id: DialogResourceId,
    dialog: vera20k_gui::DialogId,
    /// A box over whatever screen is up (the WOL page dialogs): it stays on
    /// top, with its widgets, through the screen's pushes, pops and resets,
    /// until its owner [`DialogController::close`]s it.
    overlay: bool,
}

/// LIFO dialog stack. Keys go to the top dialog's widgets through
/// [`Self::on_input`]: its `Default`/`Cancel` buttons take Enter and Escape.
pub struct DialogController {
    gui: Gui,
    stack: Vec<Instance>,
    epoch: Instant,
    /// Events of the screen's nodes, waiting for that screen to drain them.
    pending: Vec<UiEvent>,
}

impl Default for DialogController {
    fn default() -> Self {
        // A limit, not an allocation: the match's chrome layer holds the
        // sidebar's and the command bar's pictures too (a tall screen's
        // power bar and cameo rows are hundreds).
        let mut gui = Gui::new(16384);
        gui.set_base_scale(crate::ui::shell::geom::RETAIL_SCALE);
        let (time, width, height) = host_double_click_limits();
        gui.set_double_click(time.as_millis() as u64, width, height);
        gui.set_byte_len(|text| crate::util::native_string::acp_encode(text).len());
        Self { gui, stack: Vec::new(), epoch: Instant::now(), pending: Vec::new() }
    }
}

impl std::fmt::Debug for DialogController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.stack.iter().map(|i| i.id)).finish()
    }
}

impl DialogController {
    pub fn push(&mut self, id: DialogResourceId) {
        self.push_kind(id, vera20k_gui::DialogKind::Page, false);
    }

    /// [`Self::push`] as a modal: it blocks every dialog beneath it.
    pub fn push_modal(&mut self, id: DialogResourceId) {
        self.push_kind(id, vera20k_gui::DialogKind::Modal, false);
    }

    /// [`Self::push`] as an overlay: it stays on top until [`Self::close`].
    pub fn push_overlay(&mut self, id: DialogResourceId) {
        self.push_kind(id, vera20k_gui::DialogKind::Page, true);
    }

    /// [`Self::push_overlay`] as a modal: it takes every input over the
    /// screen, which keeps its own stack beneath (VERA's enlarged preview).
    pub fn push_modal_overlay(&mut self, id: DialogResourceId) {
        self.push_kind(id, vera20k_gui::DialogKind::Modal, true);
    }

    /// A dialog goes under the overlays, an overlay on top.
    fn push_kind(&mut self, id: DialogResourceId, kind: vera20k_gui::DialogKind, overlay: bool) {
        let root = self.gui.create(NodeKind::Dialog, None).expect("the shell stack stays far below the node budget");
        let dialog = self.gui.push_dialog(root, kind).expect("a fresh Dialog node");
        let at = match overlay {
            true => self.stack.len(),
            false => self.stack.iter().position(|i| i.overlay).unwrap_or(self.stack.len()),
        };
        self.stack.insert(at, Instance { id, dialog, overlay });
        for inst in &self.stack[at + 1..] {
            let _ = self.gui.raise_dialog(inst.dialog);
        }
    }

    /// Pops the topmost dialog under the overlays.
    pub fn pop(&mut self) -> Option<DialogResourceId> {
        let at = self.stack.iter().rposition(|i| !i.overlay)?;
        Some(self.remove(at))
    }

    /// Closes the topmost instance of `id`, an overlay included.
    pub fn close(&mut self, id: DialogResourceId) {
        if let Some(at) = self.stack.iter().rposition(|i| i.id == id) {
            self.remove(at);
        }
    }

    fn remove(&mut self, at: usize) -> DialogResourceId {
        let inst = self.stack.remove(at);
        let _ = self.gui.pop_dialog(inst.dialog);
        self.pending.clear();
        inst.id
    }

    /// The GUI tree the dialogs live in. A screen builds its own widgets
    /// under [`Self::root_in`] and reads them back for painting.
    pub fn gui(&self) -> &Gui {
        &self.gui
    }

    pub fn gui_mut(&mut self) -> &mut Gui {
        &mut self.gui
    }

    /// Gives the top dialog's keyboard focus to `node` (an `Edit`), or none;
    /// an edit taking it restarts its caret blink.
    pub fn focus_edit(&mut self, node: Option<NodeId>) {
        let now = self.now();
        let _ = self.gui.focus_edit(node, now);
    }

    /// Whether `node`'s caret shows now (focused, in the blink's shown half).
    /// An overlay keeps the caret of the screen under it.
    pub fn caret_visible(&self, node: NodeId) -> bool {
        let from = self.stack.iter().rposition(|i| !i.overlay).unwrap_or(0);
        self.stack[from..].iter().any(|i| self.gui.caret_visible_in(i.dialog, node, self.now()))
    }

    /// [`Self::caret_visible`] for `dialog`'s own focus, under a box too
    /// (BB keeps painting its edits' caret under its message box).
    pub fn caret_visible_in(&self, dialog: DialogResourceId, node: NodeId) -> bool {
        self.instance(dialog)
            .is_some_and(|inst| self.gui.caret_visible_in(inst.dialog, node, self.now()))
    }

    /// Control `id` under the topmost live instance of `dialog`.
    pub fn control(&self, dialog: DialogResourceId, id: u16) -> Option<NodeId> {
        self.gui.find_id(self.root_in(dialog)?, i32::from(id))
    }

    /// Control `id`'s `prop` on `dialog`.
    pub fn prop(&self, dialog: DialogResourceId, id: u16, prop: Prop) -> Option<&Value> {
        self.gui.get(self.control(dialog, id)?, prop).ok().flatten()
    }

    /// Control `id`'s boolean `prop` on `dialog` (`default` when unset).
    pub fn flag(&self, dialog: DialogResourceId, id: u16, prop: Prop, default: bool) -> bool {
        match self.prop(dialog, id, prop) {
            Some(Value::Bool(on)) => *on,
            _ => default,
        }
    }

    /// Sets `prop` on control `id` of `dialog`.
    pub fn set_prop(&mut self, dialog: DialogResourceId, id: u16, prop: Prop, value: Value) {
        if let Some(node) = self.control(dialog, id) {
            let _ = self.gui.set(node, prop, value);
        }
    }

    /// Control `id`'s text on `dialog` ("" when it has none).
    pub fn text(&self, dialog: DialogResourceId, id: u16) -> String {
        match self.prop(dialog, id, Prop::Text) {
            Some(Value::Text(text)) => text.clone(),
            _ => String::new(),
        }
    }

    /// Control `id`'s text as a program assigns it (`SetWindowTextW`): the
    /// caret goes to the end and the view to the start.
    pub fn set_text(&mut self, dialog: DialogResourceId, id: u16, text: &str) {
        self.set_prop(dialog, id, Prop::Text, Value::Text(text.to_owned()));
        self.set_prop(dialog, id, Prop::Caret, Value::Int(text.chars().count() as i32));
        self.set_prop(dialog, id, Prop::Top, Value::Int(0));
    }

    /// `SetFocus` on edit `id` of `dialog` (none clears it), on top or not.
    pub fn focus_control(&mut self, dialog: DialogResourceId, id: Option<u16>) {
        let now = self.now();
        let node = id.and_then(|id| self.control(dialog, id));
        if let Some(inst) = self.instance(dialog).map(|inst| inst.dialog) {
            let _ = self.gui.focus_edit_in(inst, node, now);
        }
    }

    /// The control id of the edit holding `dialog`'s focus.
    pub fn focused_control(&self, dialog: DialogResourceId) -> Option<u16> {
        let node = self.gui.focus_in(self.instance(dialog)?.dialog)?;
        self.gui.id_of(node).and_then(|id| u16::try_from(id).ok())
    }

    /// Drops `dialog`'s capture and hover only; a box over it keeps its own.
    pub fn drop_pointer_in(&mut self, dialog: DialogResourceId) {
        if let Some(inst) = self.instance(dialog).map(|inst| inst.dialog) {
            self.gui.drop_pointer_in(inst);
        }
    }

    /// The root node of the topmost live instance of `id`.
    pub fn root_in(&self, id: DialogResourceId) -> Option<NodeId> {
        self.gui.dialog_root(self.instance(id)?.dialog)
    }

    /// Sets the device-to-authored mapping the top dialog's widgets use.
    pub fn set_scale(&mut self, scale: crate::ui::shell::geom::ShellScale) {
        if let Some(inst) = self.base() {
            self.gui.set_dialog_scale(inst.dialog, scale);
        }
    }

    /// [`Self::set_scale`] of dialog `id`, an overlay included.
    pub fn set_scale_in(&mut self, id: DialogResourceId, scale: crate::ui::shell::geom::ShellScale) {
        if let Some(inst) = self.instance(id) {
            self.gui.set_dialog_scale(inst.dialog, scale);
        }
    }

    /// Events of the screen's own widgets since the last drain, in order.
    pub fn drain_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.pending)
    }

    /// Drops the edits' pending `Cancelled` reports: an Escape the screen
    /// took for itself was no edit's cancel.
    pub fn discard_cancelled(&mut self) {
        self.pending.retain(|event| !matches!(event, UiEvent::Cancelled { .. }));
    }

    /// Wheel, keys and text for the top dialog's widgets; true when consumed.
    pub fn on_input(&mut self, event: InputEvent) -> bool {
        if self.stack.is_empty() {
            return false;
        }
        let down = matches!(event, InputEvent::PointerDown { .. });
        let consumed = self.gui.input(event, self.now()).consumed;
        self.collect();
        if down {
            self.note_edit_press();
        }
        consumed
    }

    /// A pointer message for the chrome layer under the stack, with the
    /// chrome's events: none while a dialog is up, which owns the pointer.
    pub fn chrome_input(&mut self, event: InputEvent) -> Option<(vera20k_gui::InputResult, Vec<UiEvent>)> {
        if !self.stack.is_empty() || self.gui.chrome().is_none() {
            return None;
        }
        let result = self.gui.input(event, self.now());
        Some((result, self.gui.drain_events()))
    }

    /// Android: a press that lands on an `Edit` arms the soft keyboard
    /// (`app::input::ime`, `util::android_ime`), keyed by this dialog. The pages
    /// focus their edits when they open, so only a tap should bring the
    /// keyboard up over them.
    fn note_edit_press(&self) {
        #[cfg(target_os = "android")]
        if self.gui.pressed_edit()
            && let Some(dialog) = self.top_id()
        {
            crate::util::android_ime::arm(dialog.0);
            crate::util::android_ime::request_show();
        }
    }

    /// Widget timers (list scroll repeat); the next deadline.
    pub fn poll(&mut self) -> Option<Instant> {
        let next = self.gui.poll(self.now());
        self.collect();
        next.map(|ms| self.epoch + Duration::from_millis(ms))
    }

    /// [`Self::poll`] for a screen that only repaints when a timer fired:
    /// `redraw` runs on any event, which is dropped; the next deadline.
    pub fn poll_then(&mut self, redraw: impl FnOnce()) -> Option<Instant> {
        let next = self.poll();
        if !self.drain_events().is_empty() {
            redraw();
        }
        next
    }

    /// Moves the GUI's new events to `pending`.
    fn collect(&mut self) {
        vera20k_gui::append_events(&mut self.pending, self.gui.drain_events());
    }

    /// Closes every dialog, the overlays too, and lets go of the pointer:
    /// the controller as a new one, but for the chrome layer under the
    /// dialogs, which stays with its nodes (the match's sidebar and command
    /// bar through the in-game menus).
    pub fn close_all(&mut self) {
        while let Some(inst) = self.stack.pop() {
            let _ = self.gui.pop_dialog(inst.dialog);
        }
        self.pending.clear();
        let _ = self.gui.drain_events();
        if let Some(chrome) = self.gui.chrome() {
            self.gui.drop_pointer_in(chrome);
        }
    }

    /// Clear the stack and make `id` the sole active dialog (a shell
    /// transition); the chrome layer stays.
    pub fn reset_to(&mut self, id: DialogResourceId) {
        while self.pop().is_some() {}
        self.pending.clear();
        self.push(id);
    }

    /// Make `id` the active dialog, resetting only when it is not already on top,
    /// so an in-progress press survives across the gesture.
    pub fn ensure_active(&mut self, id: DialogResourceId) {
        if self.top_id() != Some(id) {
            self.reset_to(id);
        }
    }

    /// The screen's top dialog: overlays over it do not count, as a box
    /// kept beside the stack would not.
    pub fn top_id(&self) -> Option<DialogResourceId> {
        self.base().map(|i| i.id)
    }

    /// The topmost overlay, which takes the input over the screen.
    pub fn overlay_id(&self) -> Option<DialogResourceId> {
        self.stack.iter().rev().find(|i| i.overlay).map(|i| i.id)
    }

    fn base(&self) -> Option<&Instance> {
        self.stack.iter().rev().find(|i| !i.overlay)
    }

    /// The node captured in the topmost live instance of `id`.
    pub fn pressed_node_in(&self, id: DialogResourceId) -> Option<NodeId> {
        self.gui.pressed_in(self.instance(id)?.dialog)
    }

    /// The node under the pointer in the topmost live instance of `id`.
    pub fn hovered_node_in(&self, id: DialogResourceId) -> Option<NodeId> {
        self.gui.hovered_in(self.instance(id)?.dialog)
    }

    fn instance(&self, id: DialogResourceId) -> Option<&Instance> {
        self.stack.iter().rev().find(|i| i.id == id)
    }

    /// Give every live instance of `id` fresh press/hover state (a new
    /// dialog instance in the same stack slot).
    pub fn restart_instance(&mut self, id: DialogResourceId) {
        for inst in self.stack.iter().filter(|i| i.id == id) {
            self.gui.reset_pointer(inst.dialog);
        }
    }

    pub fn on_pointer_down(&mut self, x: i32, y: i32) {
        self.pointer(InputEvent::PointerDown { x, y, button: MouseButton::Left });
    }

    pub fn on_pointer_up(&mut self, x: i32, y: i32) {
        self.pointer(InputEvent::PointerUp { x, y, button: MouseButton::Left });
    }

    pub fn on_pointer_move(&mut self, x: i32, y: i32) {
        self.pointer(InputEvent::PointerMove { x, y });
    }

    /// A pointer message for the widgets, while a screen dialog is up.
    fn pointer(&mut self, event: InputEvent) {
        if self.base().is_some() {
            let down = matches!(event, InputEvent::PointerDown { .. });
            self.gui.input(event, self.now());
            self.collect();
            if down {
                self.note_edit_press();
            }
        }
    }

    /// The widgets' clock (ms since the controller started).
    pub fn now(&self) -> u64 {
        Instant::now().saturating_duration_since(self.epoch).as_millis() as u64
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    const A: DialogResourceId = DialogResourceId(0x00E2);
    const B: DialogResourceId = DialogResourceId(0x0100);

    #[test]
    fn keys_reach_the_top_dialogs_cancel_button_through_the_one_adapter() {
        let mut c = DialogController::default();
        c.ensure_active(A);
        let back = c.gui.create(NodeKind::Button, c.root_in(A)).unwrap();
        c.gui.set(back, Prop::Cancel, Value::Bool(true)).unwrap();
        c.push(B);
        let escape = gui_input(Some(KeyCode::Escape), false, Some("\u{1b}")).unwrap();
        assert!(!c.on_input(escape.clone()), "the box over it has no Cancel");
        c.pop();
        assert!(c.on_input(escape));
        assert_eq!(c.drain_events(), [UiEvent::Activated { node: back }]);
        assert_eq!(gui_key(KeyCode::Tab, true), Some(Key::BackTab));
        assert_eq!(gui_input(Some(KeyCode::KeyA), false, Some("a")), Some(InputEvent::Text("a".into())));
    }

    #[test]
    fn an_overlay_stays_on_top_with_its_widgets_through_resets() {
        let page = DialogResourceId(0x00CF);
        let mut c = DialogController::default();
        c.ensure_active(A);
        c.push_overlay(page);
        let edit = c.gui.create(NodeKind::Edit, c.root_in(page)).unwrap();
        c.ensure_active(B);
        c.push(A);
        // The screen's own queries see its box, as when the page lived
        // beside the stack; the GUI routes the input to the page.
        assert_eq!((c.top_id(), c.overlay_id()), (Some(A), Some(page)));
        assert_eq!(c.gui.top_dialog(), c.instance(page).map(|i| i.dialog));
        assert_eq!(c.pop(), Some(A));
        assert!(c.gui.tree().kind(edit).is_ok(), "the overlay kept its widgets");
        c.close(page);
        assert_eq!((c.top_id(), c.root_in(page)), (Some(B), None));
    }

    /// The in-game menus' reset closes their dialogs and keeps the match's
    /// chrome with its nodes, so a frame after it draws the same sidebar.
    #[test]
    fn resets_keep_the_chrome_layer() {
        let mut c = DialogController::default();
        let root = c.gui.create(NodeKind::Dialog, None).unwrap();
        let chrome = c.gui.push_chrome(root, 1024 * 768).unwrap();
        let button = c.gui.create(NodeKind::Button, Some(root)).unwrap();
        c.ensure_active(A);
        c.push_overlay(B);
        c.close_all();
        assert_eq!((c.top_id(), c.overlay_id()), (None, None));
        assert_eq!(c.gui.chrome(), Some(chrome));
        assert!(c.gui.tree().kind(button).is_ok());
        c.reset_to(A);
        c.ensure_active(B);
        assert_eq!(c.gui.chrome(), Some(chrome));
        assert!(c.gui.tree().kind(button).is_ok());
    }

    #[test]
    fn push_over_base_pops_back_to_it() {
        let modal = DialogResourceId(0x0120);
        let mut c = DialogController::default();
        c.ensure_active(A);
        c.push(modal);
        assert_eq!(c.top_id(), Some(modal));
        assert_eq!(c.pop(), Some(modal));
        assert_eq!(c.top_id(), Some(A), "LIFO pop restores the shell beneath");
    }

    #[test]
    fn ensure_active_resets_only_on_dialog_change() {
        let mut c = DialogController::default();
        c.ensure_active(A);
        let button = c.gui.create(NodeKind::Button, c.root_in(A)).unwrap();
        let _ = c.gui.set(button, Prop::Rect, Value::Rect(vera20k_gui::Rect::new(0, 0, 10, 10)));
        c.on_pointer_down(5, 5);
        assert_eq!(c.pressed_node_in(A), Some(button));
        c.ensure_active(A); // same dialog -> no reset
        assert_eq!(c.pressed_node_in(A), Some(button), "the press survives the gesture");
        c.ensure_active(B); // changed -> reset
        assert_eq!(c.pressed_node_in(B), None);
    }
}
