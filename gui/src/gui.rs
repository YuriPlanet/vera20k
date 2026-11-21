//! `Gui`: the tree plus dialog stack, pointer capture and the event queue
//! (docs/gui-library-api.md §2.1, §3, §6, §8).
//!
//! Button semantics are the shell's owner-draw button (612B70): a press
//! captures the button, the pushed look shows only while the pointer is still
//! over it, and release activates only on the captured button. A disabled
//! button still takes hover (it shows tooltip and status text) but no press.

use crate::Rect;
use crate::node::{NodeId, NodeKind, Prop, Refused, Tree, Value};
use crate::shell::geom::ShellScale;
use crate::shell::trackbar::{TrackbarPress, TrackbarRange};
use crate::widget::{self, Capture, Ctx};

/// Milliseconds on the UI clock (§9.1). The host supplies it, so tests and
/// replays control time.
pub type UiMs = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Backspace,
    Delete,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    PointerDown { x: i32, y: i32, button: MouseButton },
    PointerUp { x: i32, y: i32, button: MouseButton },
    PointerMove { x: i32, y: i32 },
    /// Wheel notches, already converted by the host (§8.8); positive is up.
    Wheel { lines: i32 },
    Key(Key),
    /// Typed text for the focused `Edit` (IME commits included).
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InputResult {
    pub consumed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    Activated { node: NodeId },
    Hovered { node: NodeId, entered: bool },
    /// Checkbox (`Bool`) or Trackbar (`Int`). `final_` is false for the
    /// intermediate positions of a drag and true once the user lets go.
    Changed { node: NodeId, value: Value, final_: bool },
    /// A press the node took (the host plays its press cue).
    Pressed { node: NodeId },
    /// List: the user picked a row (never for a host `set`).
    ListSelected { node: NodeId, row: u32 },
    ListDoubleClicked { node: NodeId, row: u32 },
    /// List or combo popup: the user scrolled to `top`.
    Scrolled { node: NodeId, top: i32 },
    /// Combo: the drop-down opened or closed (the host plays the cues).
    ComboOpened { node: NodeId },
    ComboClosed { node: NodeId },
    /// Edit.
    TextChanged { node: NodeId, text: String },
    Submitted { node: NodeId, text: String },
    Cancelled { node: NodeId },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// A full screen; only the top page takes input.
    Page,
    /// Blocks everything beneath it.
    Modal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogId(u32);

/// Pointer state is per dialog: a dialog under a modal keeps painting its own
/// captured button (the controller's per-instance state).
#[derive(Clone, Copy, Default)]
struct Pointer {
    pressed: Option<NodeId>,
    hovered: Option<NodeId>,
    /// The captured trackbar follows the pointer (a press on its thumb).
    dragging: bool,
    /// An open combo drop-down; it takes the dialog's pointer input first.
    popup: Option<NodeId>,
    /// The keyboard focus (an `Edit`).
    focus: Option<NodeId>,
    /// The node the last press landed on: Windows pairs a double-click only
    /// within one window, so a press elsewhere starts a list's sequence anew.
    last_down: Option<NodeId>,
}

struct Dialog {
    id: DialogId,
    root: NodeId,
    kind: DialogKind,
    default_button: Option<NodeId>,
    cancel_button: Option<NodeId>,
    pointer: Pointer,
    /// Maps the dialog's device rects back to the authored 800x600 numbers
    /// the native widget arithmetic runs on.
    scale: ShellScale,
}

pub struct Gui {
    tree: Tree,
    dialogs: Vec<Dialog>,
    next_dialog: u32,
    events: Vec<UiEvent>,
    /// List double-click limits: milliseconds within a width x height
    /// rectangle of the previous press.
    double_click: (UiMs, i32, i32),
    /// The text's length in the host's code page, for `MaxBytes`.
    byte_len: fn(&str) -> usize,
}

impl Gui {
    pub fn new(max_nodes: usize) -> Self {
        Gui {
            tree: Tree::new(max_nodes),
            dialogs: Vec::new(),
            next_dialog: 0,
            events: Vec::new(),
            // The host policy off Windows (`host_double_click_limits` in the
            // engine's `saved_file_input`).
            double_click: (500, 4, 4),
            byte_len: str::len,
        }
    }

    /// How many bytes a text takes in the host's code page (`MaxBytes`);
    /// UTF-8's until set.
    pub fn set_byte_len(&mut self, byte_len: fn(&str) -> usize) {
        self.byte_len = byte_len;
    }

    /// The host's double-click time and rectangle (Windows: GetDoubleClickTime,
    /// SM_CXDOUBLECLK, SM_CYDOUBLECLK).
    pub fn set_double_click(&mut self, ms: UiMs, width: i32, height: i32) {
        self.double_click = (ms, width, height);
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn create(&mut self, kind: NodeKind, parent: Option<NodeId>) -> Result<NodeId, Refused> {
        self.tree.create(kind, parent)
    }

    /// A node disabled while it holds the pointer loses the capture
    /// (`EnableWindow(FALSE)`): it never paints pressed and its release
    /// activates nothing.
    pub fn set(&mut self, id: NodeId, prop: Prop, value: Value) -> Result<(), Refused> {
        let disables = prop == Prop::Enabled && value == Value::Bool(false);
        let hides = prop == Prop::Visible && value == Value::Bool(false);
        self.tree.set(id, prop, value)?;
        if disables {
            for d in self.dialogs.iter_mut().filter(|d| d.pointer.pressed == Some(id)) {
                d.pointer.pressed = None;
                d.pointer.dragging = false;
            }
        }
        // A hidden combo's open list closes with it, without a cue.
        if hides {
            for d in self.dialogs.iter_mut().filter(|d| d.pointer.popup == Some(id)) {
                d.pointer.popup = None;
                widget::combo::dismiss(&mut self.tree, id);
            }
        }
        Ok(())
    }

    pub fn get(&self, id: NodeId, prop: Prop) -> Result<Option<&Value>, Refused> {
        self.tree.get(id, prop)
    }

    pub fn remove(&mut self, id: NodeId) -> Result<(), Refused> {
        self.tree.remove(id)?;
        let tree = &self.tree;
        self.dialogs.retain(|d| tree.kind(d.root).is_ok());
        for d in &mut self.dialogs {
            for slot in [&mut d.pointer.pressed, &mut d.pointer.hovered, &mut d.pointer.popup, &mut d.pointer.focus] {
                if slot.is_some_and(|n| tree.kind(n).is_err()) {
                    *slot = None;
                }
            }
            d.pointer.dragging &= d.pointer.pressed.is_some();
        }
        Ok(())
    }

    /// Pushes a dialog whose root is an existing `Dialog` node, with clean
    /// pointer state.
    pub fn push_dialog(
        &mut self,
        root: NodeId,
        kind: DialogKind,
        default_button: Option<NodeId>,
        cancel_button: Option<NodeId>,
    ) -> Result<DialogId, Refused> {
        if self.tree.kind(root)? != NodeKind::Dialog {
            return Err(Refused::WrongKind);
        }
        for b in [default_button, cancel_button].into_iter().flatten() {
            self.tree.kind(b)?;
        }
        let id = DialogId(self.next_dialog);
        self.next_dialog += 1;
        self.dialogs.push(Dialog { id, root, kind, default_button, cancel_button, pointer: Pointer::default(), scale: ShellScale::RETAIL });
        Ok(id)
    }

    /// Pops the dialog and removes its nodes.
    pub fn pop_dialog(&mut self, id: DialogId) -> Result<(), Refused> {
        let pos = self.dialogs.iter().position(|d| d.id == id).ok_or(Refused::NotFound)?;
        let d = self.dialogs.remove(pos);
        self.remove(d.root)
    }

    /// Moves the dialog to the top of the stack with its state.
    pub fn raise_dialog(&mut self, id: DialogId) -> Result<(), Refused> {
        let pos = self.dialogs.iter().position(|d| d.id == id).ok_or(Refused::NotFound)?;
        let d = self.dialogs.remove(pos);
        self.dialogs.push(d);
        Ok(())
    }

    pub fn top_dialog(&self) -> Option<DialogId> {
        self.dialogs.last().map(|d| d.id)
    }

    pub fn dialog_root(&self, id: DialogId) -> Option<NodeId> {
        self.dialogs.iter().find(|d| d.id == id).map(|d| d.root)
    }

    pub fn set_dialog_scale(&mut self, id: DialogId, scale: ShellScale) {
        if let Some(d) = self.dialogs.iter_mut().find(|d| d.id == id) {
            d.scale = scale;
        }
    }

    pub fn dialogs(&self) -> impl Iterator<Item = DialogId> + '_ {
        self.dialogs.iter().map(|d| d.id)
    }

    fn dialog(&self, id: DialogId) -> Option<&Dialog> {
        self.dialogs.iter().find(|d| d.id == id)
    }

    /// Forgets the dialog's capture and hover (a fresh dialog instance).
    pub fn reset_pointer(&mut self, id: DialogId) {
        if let Some(d) = self.dialogs.iter_mut().find(|d| d.id == id) {
            d.pointer = Pointer::default();
        }
    }

    /// [`Self::drop_pointer_in`] for the top dialog, when the host loses the
    /// pointer (`0x0061E3AF..0x0061E44E` for a held trackbar).
    pub fn cancel_pointer(&mut self) {
        if let Some(top) = self.top_dialog() {
            self.drop_pointer_in(top);
        }
    }

    /// Drops dialog `id`'s capture and hover only, on top or not: a page
    /// under a modal box loses the pointer while the box keeps its press. A
    /// held list scrollbar lets go (`0x0061D2D3`); an open drop-down stays
    /// open but lets go of a held scroll part.
    pub fn drop_pointer_in(&mut self, id: DialogId) {
        let Some(d) = self.dialogs.iter_mut().find(|d| d.id == id) else { return };
        if let Some(held) = d.pointer.pressed.take() {
            widget::list::cancel_scroll(&mut self.tree, held);
        }
        d.pointer.dragging = false;
        d.pointer.hovered = None;
        if let Some(popup) = d.pointer.popup {
            widget::combo::release_scroll(&mut self.tree, popup);
        }
    }

    /// Whether the top dialog's captured trackbar follows the pointer (the
    /// press took its thumb).
    pub fn dragging(&self) -> bool {
        self.top_pointer().dragging
    }

    pub fn pressed_in(&self, id: DialogId) -> Option<NodeId> {
        self.dialog(id).and_then(|d| d.pointer.pressed)
    }

    pub fn hovered_in(&self, id: DialogId) -> Option<NodeId> {
        self.dialog(id).and_then(|d| d.pointer.hovered)
    }

    fn top_pointer(&self) -> Pointer {
        self.dialogs.last().map(|d| d.pointer).unwrap_or_default()
    }

    pub fn pressed(&self) -> Option<NodeId> {
        self.top_pointer().pressed
    }

    pub fn hovered(&self) -> Option<NodeId> {
        self.top_pointer().hovered
    }

    /// The pushed look: captured and still under the pointer (612B70).
    pub fn shows_pressed(&self, id: NodeId) -> bool {
        let p = self.top_pointer();
        p.pressed == Some(id) && p.hovered == Some(id)
    }

    pub fn drain_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.events)
    }

    /// Routes one input to the top dialog. Down and up do not move hover:
    /// the window system sends a move before them.
    pub fn input(&mut self, event: InputEvent, now: UiMs) -> InputResult {
        let Some(top) = self.dialogs.last() else { return InputResult::default() };
        let (root, modal) = (top.root, top.kind == DialogKind::Modal);
        if let Some(popup) = top.pointer.popup.filter(|n| !self.popup_hidden(*n)) {
            match event {
                InputEvent::PointerDown { x, y, button: MouseButton::Left } => {
                    self.with_ctx(now, |ctx| widget::combo::popup_press(ctx, popup, x, y));
                    return InputResult { consumed: true };
                }
                InputEvent::PointerMove { x, y } => {
                    self.with_ctx(now, |ctx| widget::combo::popup_move(ctx, popup, x, y));
                    // The list is a window of its own: the dialog under it
                    // still hit-tests the rest of the screen (status help).
                    let covered = self.combo_drop_down(popup).is_some_and(|d| d.popup.rect.contains(x, y));
                    let over = self.hit(root, x, y).filter(|_| !covered);
                    self.set_hover(over);
                    return InputResult { consumed: true };
                }
                InputEvent::PointerUp { x, y, button: MouseButton::Left } => {
                    self.with_ctx(now, |ctx| widget::combo::popup_release(ctx, popup, x, y));
                    return InputResult { consumed: true };
                }
                _ => {}
            }
        }
        match event {
            InputEvent::PointerMove { x, y } => {
                if let Some(held) = self.top_pointer().pressed {
                    match self.tree.kind(held) {
                        Ok(NodeKind::Trackbar) => {
                            // The trackbar holds the pointer: nothing else
                            // sees it until the release (`SetCapture` 0x0061E4DF).
                            self.set_hover(None);
                            if self.top_mut().pointer.dragging {
                                self.drag_trackbar(held, x);
                            }
                            return InputResult { consumed: true };
                        }
                        Ok(NodeKind::List) => {
                            self.with_ctx(now, |ctx| widget::list::drag(ctx, held, x, y));
                        }
                        Ok(NodeKind::Edit) => {
                            self.with_ctx(now, |ctx| widget::edit::drag(ctx, held, x, y));
                        }
                        _ => {}
                    }
                }
                let over = self.hit(root, x, y);
                self.set_hover(over);
                InputResult { consumed: over.is_some() || modal }
            }
            InputEvent::PointerDown { x, y, button: MouseButton::Left } => {
                let over = self.hit_at(root, x, y, true);
                let enabled = over.filter(|n| self.tree.effective(*n, Prop::Enabled));
                self.top_mut().pointer.pressed = None;
                let before = std::mem::replace(&mut self.top_mut().pointer.last_down, over);
                if let Some(before) = before.filter(|n| Some(*n) != over) {
                    widget::list::forget_press(&mut self.tree, before);
                }
                if let Some(n) = enabled {
                    self.press(n, x, y, now);
                }
                InputResult { consumed: over.is_some() || modal }
            }
            InputEvent::PointerUp { x, y, button: MouseButton::Left } => {
                let over = self.hit_at(root, x, y, true);
                let pointer = &mut self.top_mut().pointer;
                let held = pointer.pressed.take();
                pointer.dragging = false;
                if let Some(n) = held {
                    self.release(n, x, y, over == Some(n), now);
                }
                InputResult { consumed: held.is_some() || over.is_some() || modal }
            }
            InputEvent::PointerDown { .. } | InputEvent::PointerUp { .. } => {
                InputResult { consumed: modal }
            }
            InputEvent::Wheel { lines } => {
                let target = self.top_pointer().hovered.filter(|n| self.tree.kind(*n) == Ok(NodeKind::List));
                let scrolled = target
                    .is_some_and(|n| self.with_ctx(now, |ctx| widget::list::wheel(ctx, n, lines)));
                InputResult { consumed: scrolled || modal }
            }
            InputEvent::Key(key) => {
                if let Some(f) = self.top_pointer().focus.filter(|_| widget::is_edit_key(key)) {
                    if self.with_ctx(now, |ctx| widget::edit::key(ctx, f, key)) {
                        return InputResult { consumed: true };
                    }
                }
                self.key(key)
            }
            InputEvent::Text(text) => match self.top_pointer().focus {
                Some(f) => {
                    self.with_ctx(now, |ctx| widget::edit::text(ctx, f, &text));
                    InputResult { consumed: true }
                }
                None => InputResult { consumed: modal },
            },
        }
    }

    /// Runs the widget timers (scroll-arrow repeat) of the top dialog's
    /// captured node; returns when the next poll is due.
    pub fn poll(&mut self, now: UiMs) -> Option<UiMs> {
        if let Some(popup) = self.top_pointer().popup.filter(|n| !self.popup_hidden(*n)) {
            return self.with_ctx(now, |ctx| widget::combo::poll(ctx, popup));
        }
        let held = self.top_pointer().pressed?;
        match self.tree.kind(held) {
            Ok(NodeKind::List) => self.with_ctx(now, |ctx| widget::list::poll(ctx, held)),
            _ => None,
        }
    }

    pub fn focus(&self) -> Option<NodeId> {
        self.top_pointer().focus
    }

    /// Whether the host paints `node`'s caret now: it has the top dialog's
    /// focus and is in the shown half of NewEdit's two-second blink.
    pub fn caret_visible(&self, node: NodeId, now: UiMs) -> bool {
        self.dialogs.last().is_some_and(|d| self.caret_visible_in(d.id, node, now))
    }

    /// [`Gui::caret_visible`] for `dialog`'s own focus, which a dialog under
    /// a modal keeps.
    pub fn caret_visible_in(&self, dialog: DialogId, node: NodeId, now: UiMs) -> bool {
        self.dialog(dialog).is_some_and(|d| d.pointer.focus == Some(node))
            && self.tree.node(node).is_ok_and(|n| match &n.state {
                widget::WidgetState::Edit(s) => s.caret_visible(now),
                _ => widget::edit::EditState::default().caret_visible(now),
            })
    }

    /// Gives the top dialog's keyboard focus to `node` (an `Edit`), or none.
    pub fn set_focus(&mut self, node: Option<NodeId>) -> Result<(), Refused> {
        if let Some(n) = node {
            if self.tree.kind(n)? != NodeKind::Edit {
                return Err(Refused::WrongKind);
            }
        }
        match self.dialogs.last_mut() {
            Some(d) => {
                d.pointer.focus = node;
                Ok(())
            }
            None => Err(Refused::NotFound),
        }
    }

    /// [`Gui::set_focus`] as the screen's own `SetFocus`: an edit that takes
    /// the focus from elsewhere restarts NewEdit's caret blink, as a press
    /// does.
    pub fn focus_edit(&mut self, node: Option<NodeId>, now: UiMs) -> Result<(), Refused> {
        let top = self.dialogs.last().map_or(DialogId(u32::MAX), |d| d.id);
        self.focus_edit_in(top, node, now)
    }

    /// The edit holding dialog `id`'s keyboard focus, on top or not.
    pub fn focus_in(&self, id: DialogId) -> Option<NodeId> {
        self.dialog(id).and_then(|d| d.pointer.focus)
    }

    /// [`Gui::focus_edit`] in dialog `id`, which need not be on top: a page
    /// under a modal keeps the focus it is given for when the modal closes.
    pub fn focus_edit_in(&mut self, id: DialogId, node: Option<NodeId>, now: UiMs) -> Result<(), Refused> {
        if let Some(n) = node.filter(|n| self.tree.kind(*n) != Ok(NodeKind::Edit)) {
            self.tree.kind(n)?;
            return Err(Refused::WrongKind);
        }
        let d = self.dialogs.iter_mut().find(|d| d.id == id).ok_or(Refused::NotFound)?;
        let before = std::mem::replace(&mut d.pointer.focus, node);
        if let Some(n) = node.filter(|n| before != Some(*n)) {
            self.tree.node_mut(n)?.state = widget::WidgetState::Edit(widget::edit::EditState { focused_at: now });
        }
        Ok(())
    }

    /// The open combo drop-down of the top dialog.
    pub fn popup(&self) -> Option<NodeId> {
        self.top_pointer().popup
    }

    /// Closes the top dialog's open drop-down for the host (no event, as a
    /// host `set` reports none); true when one was open.
    pub fn close_popup(&mut self) -> bool {
        let Some(node) = self.dialogs.last_mut().and_then(|d| d.pointer.popup.take()) else {
            return false;
        };
        widget::combo::dismiss(&mut self.tree, node);
        true
    }

    /// Opens combo `node`'s list as the top dialog's drop-down for the host
    /// (a button that drops it), `top` its first shown row: no cue, no event.
    pub fn open_popup(&mut self, node: NodeId, top: usize) -> Result<(), Refused> {
        if self.tree.kind(node)? != NodeKind::Combo {
            return Err(Refused::WrongKind);
        }
        let d = self.dialogs.last_mut().ok_or(Refused::NotFound)?;
        d.pointer.popup = Some(node);
        widget::combo::open_for_host(&mut self.tree, node, top);
        Ok(())
    }

    /// The wheel for the top dialog's open drop-down, for a host whose lists
    /// take it (VERA's roster): `rows` on when the pointer at `x`, `y` is
    /// over the list. True when a list is open, which then owns the wheel.
    pub fn wheel_popup(&mut self, rows: i32, x: i32, y: i32) -> bool {
        let Some(node) = self.popup().filter(|n| !self.popup_hidden(*n)) else { return false };
        if self.combo_drop_down(node).is_some_and(|d| d.popup.rect.contains(x, y)) {
            self.with_ctx(0, |ctx| widget::combo::wheel(ctx, node, rows, x, y));
        }
        true
    }

    fn popup_hidden(&self, node: NodeId) -> bool {
        matches!(self.tree.get(node, Prop::PopupHidden), Ok(Some(Value::Bool(true))))
    }

    /// The open drop-down of combo `node` (in any dialog), on the geometry its
    /// pointer input hits, for the host to paint.
    pub fn combo_drop_down(&self, node: NodeId) -> Option<widget::combo::ComboDropDown> {
        if self.popup_hidden(node) {
            return None;
        }
        let d = self.dialogs.iter().find(|d| d.pointer.popup == Some(node))?;
        widget::combo::drop_down(&self.tree, node, d.scale)
    }

    /// What the host paints of List `node`.
    pub fn list_view(&self, node: NodeId) -> Option<widget::list::ListView> {
        widget::list::view(&self.tree, node)
    }

    fn with_ctx<R>(&mut self, now: UiMs, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let d = self.dialogs.last_mut().expect("callers check for a top dialog");
        let mut ctx = Ctx {
            tree: &mut self.tree,
            events: &mut self.events,
            scale: d.scale,
            now,
            double_click: self.double_click,
            byte_len: self.byte_len,
            popup: &mut d.pointer.popup,
            focus: &mut d.pointer.focus,
        };
        f(&mut ctx)
    }

    /// A left press on an enabled node.
    fn press(&mut self, node: NodeId, x: i32, y: i32, now: UiMs) {
        let Ok(n) = self.tree.node(node) else { return };
        if n.kind == NodeKind::Text {
            return;
        }
        let delegated = match n.kind {
            NodeKind::List => Some(self.with_ctx(now, |ctx| widget::list::press(ctx, node, x, y))),
            NodeKind::Combo => Some(self.with_ctx(now, |ctx| widget::combo::press(ctx, node, x, y))),
            NodeKind::Edit => Some(self.with_ctx(now, |ctx| widget::edit::press(ctx, node, x, y))),
            _ => None,
        };
        if let Some(capture) = delegated {
            if capture == Capture::Yes {
                self.top_mut().pointer.pressed = Some(node);
            }
            return;
        }
        let Ok(n) = self.tree.node(node) else { return };
        if n.kind == NodeKind::Checkbox {
            // OwnerDraw_CheckBox 0x006163A0 toggles on the press, admitted
            // only on its 18x18 icon at the row's left; a `HitRect` takes it
            // anywhere.
            let rect = self.tree.screen_rect(node).unwrap_or_default();
            let scale = self.dialogs.last().map_or(ShellScale::RETAIL, |d| d.scale);
            let anywhere = n.props.contains_key(&Prop::HitRect);
            if anywhere || scale.derive_rect(rect, |r| Rect::new(r.x, r.y, 18, 18)).contains(x, y) {
                self.toggle(node);
            }
            return;
        }
        match n.kind {
            NodeKind::Trackbar => {
                let range = trackbar_range(n);
                let value = n.int_prop(Prop::Pos, range.min);
                let rect = self.tree.screen_rect(node).unwrap_or_default();
                let scale = self.trackbar_scale(n);
                match range.press_in(value, x, y, rect, scale) {
                    TrackbarPress::Miss => {}
                    TrackbarPress::Hold => self.top_mut().pointer.pressed = Some(node),
                    TrackbarPress::Drag => {
                        let pointer = &mut self.top_mut().pointer;
                        pointer.pressed = Some(node);
                        pointer.dragging = true;
                    }
                    TrackbarPress::Jump(to) => {
                        self.top_mut().pointer.pressed = Some(node);
                        self.move_trackbar(node, to);
                    }
                }
            }
            _ => {
                self.top_mut().pointer.pressed = Some(node);
                self.events.push(UiEvent::Pressed { node });
            }
        }
    }

    /// The release of a captured node; `over` when the pointer is still on it.
    fn release(&mut self, node: NodeId, x: i32, y: i32, over: bool, now: UiMs) {
        let Ok(n) = self.tree.node(node) else { return };
        let enabled = self.tree.effective(node, Prop::Enabled);
        match n.kind {
            NodeKind::List => self.with_ctx(now, |ctx| widget::list::release(ctx, node, x, y)),
            NodeKind::Combo => self.with_ctx(now, |ctx| widget::combo::release(ctx, node, x, y)),
            NodeKind::Edit => self.with_ctx(now, |ctx| widget::edit::release(ctx, node, x, y)),
            NodeKind::Trackbar => {
                let value = Value::Int(n.int_prop(Prop::Pos, 0));
                self.events.push(UiEvent::Changed { node, value, final_: true });
            }
            _ if over && enabled => self.activate(node),
            _ => {}
        }
    }

    fn toggle(&mut self, node: NodeId) {
        let checked = !self.tree.node(node).is_ok_and(|n| n.bool_prop(Prop::Checked, false));
        let _ = self.tree.set(node, Prop::Checked, Value::Bool(checked));
        self.events.push(UiEvent::Changed { node, value: Value::Bool(checked), final_: true });
    }

    fn drag_trackbar(&mut self, node: NodeId, x: i32) {
        let Ok(n) = self.tree.node(node) else { return };
        let range = trackbar_range(n);
        let rect = self.tree.screen_rect(node).unwrap_or_default();
        let scale = self.trackbar_scale(n);
        self.move_trackbar(node, range.value_at_in(x, rect, scale));
    }

    /// The scale trackbar `n` hits at: its `ScaleScreen`'s, else its dialog's.
    fn trackbar_scale(&self, n: &crate::node::Node) -> ShellScale {
        match n.props.get(&Prop::ScaleScreen) {
            Some(Value::Rect(r)) => ShellScale::from_screen(r.w, r.h),
            _ => self.dialogs.last().map_or(ShellScale::RETAIL, |d| d.scale),
        }
    }

    fn move_trackbar(&mut self, node: NodeId, to: i32) {
        let from = self.tree.node(node).map_or(to, |n| n.int_prop(Prop::Pos, trackbar_range(n).min));
        if from != to {
            let _ = self.tree.set(node, Prop::Pos, Value::Int(to));
            self.events.push(UiEvent::Changed { node, value: Value::Int(to), final_: false });
        }
    }

    fn top_mut(&mut self) -> &mut Dialog {
        self.dialogs.last_mut().expect("callers check for a top dialog")
    }

    fn key(&mut self, key: Key) -> InputResult {
        let Some(top) = self.dialogs.last() else { return InputResult::default() };
        let target = match key {
            Key::Enter => top.default_button,
            Key::Escape => top.cancel_button,
            _ => None,
        };
        let modal = top.kind == DialogKind::Modal;
        match target {
            Some(n) if self.tree.effective(n, Prop::Enabled) => {
                self.activate(n);
                InputResult { consumed: true }
            }
            _ => InputResult { consumed: modal },
        }
    }

    fn activate(&mut self, node: NodeId) {
        self.events.push(UiEvent::Activated { node });
    }

    fn set_hover(&mut self, over: Option<NodeId>) {
        let pointer = &mut self.top_mut().pointer;
        if pointer.hovered == over {
            return;
        }
        let old = pointer.hovered;
        pointer.hovered = over;
        if let Some(old) = old {
            self.events.push(UiEvent::Hovered { node: old, entered: false });
        }
        if let Some(new) = over {
            self.events.push(UiEvent::Hovered { node: new, entered: true });
        }
    }

    /// The interactive node under the point within `root`: visible, of an
    /// interactive kind, first in tree order (§8.7; the shell's controls do
    /// not overlap). Disabled nodes still hit, for hover.
    fn hit(&self, root: NodeId, x: i32, y: i32) -> Option<NodeId> {
        self.hit_at(root, x, y, false)
    }

    /// [`Self::hit`] for the host, under any open drop-down; `press` skips
    /// `PressThrough` nodes, as a press and its release do.
    pub fn hit_at(&self, root: NodeId, x: i32, y: i32, press: bool) -> Option<NodeId> {
        let mut found = None;
        self.walk(root, &mut |id, kind| {
            // A static with status help hovers too (it takes no press).
            let help = kind == NodeKind::Text && self.tree.get(id, Prop::Tooltip).is_ok_and(|t| t.is_some());
            let through = press && matches!(self.tree.get(id, Prop::PressThrough), Ok(Some(Value::Bool(true))));
            if found.is_none()
                && !through
                && (kind.is_focusable() || help)
                && self.tree.effective(id, Prop::Visible)
                && self.tree.screen_hit_rect(id).is_ok_and(|r| r.contains(x, y))
            {
                found = Some(id);
            }
        });
        found
    }

    fn walk(&self, id: NodeId, f: &mut impl FnMut(NodeId, NodeKind)) {
        let Ok(kind) = self.tree.kind(id) else { return };
        f(id, kind);
        if let Ok(children) = self.tree.children(id) {
            for c in children {
                self.walk(*c, f);
            }
        }
    }
}

/// The trackbar's range; without its value plaque the track runs to the
/// window's right end (`0x4AC` with 0).
fn trackbar_range(n: &crate::node::Node) -> TrackbarRange {
    TrackbarRange::new(n.int_prop(Prop::Min, 0), n.int_prop(Prop::Max, 100), n.int_prop(Prop::Step, 1))
        .with_plaque(n.bool_prop(Prop::ShowValue, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_with_two_buttons() -> (Gui, NodeId, NodeId) {
        let mut g = Gui::new(16);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let a = g.create(NodeKind::Button, Some(root)).unwrap();
        let b = g.create(NodeKind::Button, Some(root)).unwrap();
        g.set(a, Prop::Rect, Value::Rect(Rect::new(0, 0, 10, 10))).unwrap();
        g.set(b, Prop::Rect, Value::Rect(Rect::new(10, 0, 10, 10))).unwrap();
        g.push_dialog(root, DialogKind::Page, None, None).unwrap();
        (g, a, b)
    }

    fn down(g: &mut Gui, x: i32) {
        g.input(InputEvent::PointerMove { x, y: 5 }, 0);
        g.input(InputEvent::PointerDown { x, y: 5, button: MouseButton::Left }, 0);
    }
    fn up(g: &mut Gui, x: i32) {
        g.input(InputEvent::PointerMove { x, y: 5 }, 0);
        g.input(InputEvent::PointerUp { x, y: 5, button: MouseButton::Left }, 0);
    }
    fn activations(g: &mut Gui) -> Vec<NodeId> {
        g.drain_events()
            .into_iter()
            .filter_map(|e| match e {
                UiEvent::Activated { node, .. } => Some(node),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn press_must_match_release() {
        let (mut g, a, _) = page_with_two_buttons();
        down(&mut g, 5);
        up(&mut g, 15);
        assert!(activations(&mut g).is_empty());
        down(&mut g, 5);
        up(&mut g, 5);
        assert_eq!(activations(&mut g), vec![a]);
    }

    #[test]
    fn dragging_off_drops_the_pushed_look_but_keeps_capture() {
        let (mut g, a, _) = page_with_two_buttons();
        down(&mut g, 5);
        assert!(g.shows_pressed(a));
        g.input(InputEvent::PointerMove { x: 50, y: 50 }, 0);
        assert!(!g.shows_pressed(a));
        assert_eq!(g.pressed(), Some(a));
        g.input(InputEvent::PointerMove { x: 5, y: 5 }, 0);
        assert!(g.shows_pressed(a));
    }

    #[test]
    fn disabled_button_hovers_but_never_activates() {
        let (mut g, _, b) = page_with_two_buttons();
        g.set(b, Prop::Enabled, Value::Bool(false)).unwrap();
        g.input(InputEvent::PointerMove { x: 15, y: 5 }, 7);
        assert_eq!(g.hovered(), Some(b));
        down(&mut g, 15);
        assert_eq!(g.pressed(), None);
        up(&mut g, 15);
        assert!(activations(&mut g).is_empty());
    }

    #[test]
    fn modal_on_top_blocks_the_page() {
        let (mut g, a, _) = page_with_two_buttons();
        let m = g.create(NodeKind::Dialog, None).unwrap();
        let ok = g.create(NodeKind::Button, Some(m)).unwrap();
        g.set(ok, Prop::Rect, Value::Rect(Rect::new(100, 100, 10, 10))).unwrap();
        let id = g.push_dialog(m, DialogKind::Modal, Some(ok), Some(ok)).unwrap();
        down(&mut g, 5);
        up(&mut g, 5);
        assert!(activations(&mut g).is_empty(), "page button under a modal");
        assert!(g.input(InputEvent::Key(Key::Enter), 0).consumed);
        assert_eq!(activations(&mut g), vec![ok]);
        g.pop_dialog(id).unwrap();
        assert_eq!(g.pressed(), None, "the page's own state survived the modal");
        down(&mut g, 5);
        up(&mut g, 5);
        assert_eq!(activations(&mut g), vec![a]);
    }

    #[test]
    fn checkbox_takes_a_press_only_on_its_icon() {
        let (mut g, _, _) = page_with_two_buttons();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let cb = g.create(NodeKind::Checkbox, Some(root)).unwrap();
        g.set(cb, Prop::Rect, Value::Rect(Rect::new(30, 0, 100, 20))).unwrap();
        down(&mut g, 30 + 18);
        assert_eq!(g.hovered(), Some(cb), "the caption still hovers");
        assert_eq!(g.get(cb, Prop::Checked).unwrap(), None);
        down(&mut g, 30 + 17);
        assert_eq!(g.get(cb, Prop::Checked).unwrap(), Some(&Value::Bool(true)), "toggled on the press");
        up(&mut g, 30 + 17);
        assert_eq!(g.get(cb, Prop::Checked).unwrap(), Some(&Value::Bool(true)), "not again on the release");
    }

    #[test]
    fn trackbar_jumps_beside_the_thumb_and_drags_only_from_it() {
        let mut g = Gui::new(8);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let bar = g.create(NodeKind::Trackbar, Some(root)).unwrap();
        // 200 px wide, 20 tall: track 1..=138, plaque the last 50 px.
        g.set(bar, Prop::Rect, Value::Rect(Rect::new(0, 0, 200, 20))).unwrap();
        g.set(bar, Prop::Max, Value::Int(10)).unwrap();
        g.push_dialog(root, DialogKind::Page, None, None).unwrap();
        let at = |g: &mut Gui, e: InputEvent| {
            g.input(e, 0);
        };
        // Beside the thumb, in the admitted strip: one jump, no drag after.
        at(&mut g, InputEvent::PointerDown { x: 100, y: 15, button: MouseButton::Left });
        let jumped = g.get(bar, Prop::Pos).unwrap().cloned();
        assert!(matches!(jumped, Some(Value::Int(v)) if v > 0));
        at(&mut g, InputEvent::PointerMove { x: 10, y: 15 });
        assert_eq!(g.get(bar, Prop::Pos).unwrap().cloned(), jumped);
        at(&mut g, InputEvent::PointerUp { x: 10, y: 15, button: MouseButton::Left });
        // Above the strip: capture only.
        at(&mut g, InputEvent::PointerDown { x: 20, y: 1, button: MouseButton::Left });
        assert_eq!(g.get(bar, Prop::Pos).unwrap().cloned(), jumped);
        at(&mut g, InputEvent::PointerUp { x: 20, y: 1, button: MouseButton::Left });
        let finals = g
            .drain_events()
            .into_iter()
            .filter(|e| matches!(e, UiEvent::Changed { final_: true, .. }))
            .count();
        assert_eq!(finals, 2, "every release reports the final position");
    }

    #[test]
    fn a_hit_rect_checkbox_toggles_anywhere_in_it_and_a_plaqueless_track_runs_to_the_end() {
        let (mut g, _, _) = page_with_two_buttons();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let cb = g.create(NodeKind::Checkbox, Some(root)).unwrap();
        g.set(cb, Prop::Rect, Value::Rect(Rect::new(60, 0, 20, 10))).unwrap();
        g.set(cb, Prop::HitRect, Value::Rect(Rect::new(30, 0, 100, 10))).unwrap();
        down(&mut g, 31);
        assert_eq!(g.get(cb, Prop::Checked).unwrap(), Some(&Value::Bool(true)));
        // 120 px, 0..=2, no plaque: the track spans the whole window
        // (`0x0061D950` with a zero reserve), so its middle is the middle
        // value; a 50 px plaque would make it the top one.
        let bar = g.create(NodeKind::Trackbar, Some(root)).unwrap();
        g.set(bar, Prop::Rect, Value::Rect(Rect::new(0, 20, 120, 20))).unwrap();
        g.set(bar, Prop::Max, Value::Int(2)).unwrap();
        g.set(bar, Prop::ShowValue, Value::Bool(false)).unwrap();
        g.input(InputEvent::PointerDown { x: 60, y: 35, button: MouseButton::Left }, 0);
        assert_eq!(g.get(bar, Prop::Pos).unwrap(), Some(&Value::Int(1)));
        // Losing the pointer drops the hold.
        g.cancel_pointer();
        assert_eq!((g.pressed(), g.dragging()), (None, false));
    }

    #[test]
    fn disabling_a_held_button_drops_its_pushed_look_and_its_activation() {
        let (mut g, a, _) = page_with_two_buttons();
        down(&mut g, 5);
        assert!(g.shows_pressed(a));
        g.set(a, Prop::Enabled, Value::Bool(false)).unwrap();
        assert!(!g.shows_pressed(a) && g.pressed().is_none());
        g.set(a, Prop::Enabled, Value::Bool(true)).unwrap();
        up(&mut g, 5);
        assert!(activations(&mut g).is_empty());
    }

    #[test]
    fn a_static_with_help_hovers_but_takes_no_press() {
        let (mut g, _, _) = page_with_two_buttons();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let text = g.create(NodeKind::Text, Some(root)).unwrap();
        g.set(text, Prop::Rect, Value::Rect(Rect::new(30, 0, 10, 10))).unwrap();
        down(&mut g, 35);
        assert_eq!(g.hovered(), None, "a static without help is not hit");
        g.set(text, Prop::Tooltip, Value::Text("STT:X".into())).unwrap();
        down(&mut g, 35);
        assert_eq!((g.hovered(), g.pressed()), (Some(text), None));
        up(&mut g, 35);
        assert!(activations(&mut g).is_empty());
    }

    /// A page with an `Edit` at 0..100 x 0..10 and an OK/Cancel pair.
    fn page_with_edit() -> (Gui, NodeId, NodeId, NodeId) {
        let mut g = Gui::new(8);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let edit = g.create(NodeKind::Edit, Some(root)).unwrap();
        let ok = g.create(NodeKind::Button, Some(root)).unwrap();
        let cancel = g.create(NodeKind::Button, Some(root)).unwrap();
        g.set(edit, Prop::Rect, Value::Rect(Rect::new(0, 0, 100, 10))).unwrap();
        g.push_dialog(root, DialogKind::Page, Some(ok), Some(cancel)).unwrap();
        (g, edit, ok, cancel)
    }

    fn text_of(g: &Gui, n: NodeId) -> (String, i32) {
        let text = match g.get(n, Prop::Text).unwrap() {
            Some(Value::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let caret = match g.get(n, Prop::Caret).unwrap() {
            Some(Value::Int(c)) => *c,
            _ => -1,
        };
        (text, caret)
    }

    #[test]
    fn a_programmatic_focus_restarts_the_caret_blink_only_when_it_moves() {
        let (mut g, edit, _, _) = page_with_edit();
        g.focus_edit(Some(edit), 1500).unwrap();
        assert!(g.caret_visible(edit, 1500) && !g.caret_visible(edit, 2600));
        g.focus_edit(Some(edit), 2600).unwrap();
        assert!(!g.caret_visible(edit, 2600), "focus that stays keeps the blink's phase");
    }

    #[test]
    fn edit_press_focuses_and_keys_edit_at_the_caret() {
        let (mut g, edit, _, _) = page_with_edit();
        g.set(edit, Prop::Text, Value::Text("abd".into())).unwrap();
        g.input(InputEvent::PointerDown { x: 5, y: 5, button: MouseButton::Left }, 100);
        assert_eq!(g.focus(), Some(edit));
        assert_eq!(g.pressed(), None, "NewEdit takes no capture");
        assert!(g.caret_visible(edit, 1099) && !g.caret_visible(edit, 1100));
        for key in [Key::Left, Key::Backspace, Key::Home, Key::Delete] {
            assert!(g.input(InputEvent::Key(key), 0).consumed);
        }
        g.input(InputEvent::Text("xc".into()), 0);
        assert_eq!(text_of(&g, edit), ("xcd".into(), 2));
        let changed: Vec<String> = g
            .drain_events()
            .into_iter()
            .filter_map(|e| match e {
                UiEvent::TextChanged { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(changed, ["ad", "d", "xcd"], "moves change nothing");
    }

    #[test]
    fn select_on_focus_is_replaced_by_typing_and_bytes_cap_the_text() {
        let (mut g, edit, _, _) = page_with_edit();
        g.set(edit, Prop::Text, Value::Text("Player".into())).unwrap();
        g.set(edit, Prop::SelectOnFocus, Value::Bool(true)).unwrap();
        g.set(edit, Prop::MaxBytes, Value::Int(4)).unwrap();
        g.input(InputEvent::PointerDown { x: 5, y: 5, button: MouseButton::Left }, 0);
        assert_eq!(g.get(edit, Prop::AllSelected).unwrap(), Some(&Value::Bool(true)));
        g.input(InputEvent::Text("aé\u{20ac}b".into()), 0);
        // UTF-8 bytes until the host sets its code page: é takes two, € three.
        assert_eq!(text_of(&g, edit), ("aéb".into(), 3));
        g.input(InputEvent::Key(Key::Backspace), 0);
        assert_eq!(text_of(&g, edit), ("aé".into(), 2), "the selection went with the typing");
    }

    #[test]
    fn edit_filters_typing_by_its_props() {
        let (mut g, edit, _, _) = page_with_edit();
        g.set_focus(Some(edit)).unwrap();
        g.set(edit, Prop::MaxLen, Value::Int(4)).unwrap();
        g.set(edit, Prop::ByteOnly, Value::Bool(true)).unwrap();
        g.set(edit, Prop::Rejected, Value::Text(",".into())).unwrap();
        g.input(InputEvent::Text("a,\u{100}b\tcdef".into()), 0);
        assert_eq!(text_of(&g, edit), ("abcd".into(), 4));
        g.set(edit, Prop::Text, Value::Text(String::new())).unwrap();
        g.set(edit, Prop::Allowed, Value::Text("0123456789".into())).unwrap();
        g.input(InputEvent::Text("1a2".into()), 0);
        assert_eq!(text_of(&g, edit).0, "12");
    }

    #[test]
    fn edit_enter_and_escape_report_and_still_reach_the_dialog_buttons() {
        let (mut g, edit, ok, cancel) = page_with_edit();
        g.set(edit, Prop::Text, Value::Text("hi".into())).unwrap();
        g.set_focus(Some(edit)).unwrap();
        assert!(g.input(InputEvent::Key(Key::Enter), 0).consumed);
        assert!(g.input(InputEvent::Key(Key::Escape), 0).consumed);
        assert_eq!(
            g.drain_events(),
            [
                UiEvent::Submitted { node: edit, text: "hi".into() },
                UiEvent::Activated { node: ok },
                UiEvent::Cancelled { node: edit },
                UiEvent::Activated { node: cancel },
            ]
        );
    }
}
