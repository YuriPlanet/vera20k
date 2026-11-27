//! `Gui`: the tree plus dialog stack, pointer capture and the event queue
//! (docs/gui-library-api.md §2.1, §3, §6, §8).
//!
//! Button semantics are the shell's owner-draw button (612B70): a press
//! captures the button, the pushed look shows only while the pointer is still
//! over it, and release activates only on the captured button. A disabled
//! button still takes hover (it shows tooltip and status text) but no press.

mod layers;

use std::collections::HashMap;
use std::ops::ControlFlow;

use crate::Rect;
use crate::layout::Layout;
use crate::node::{NodeId, NodeKind, Prop, Refused, Tree, Value};
use crate::origin::Origin;
use crate::shell::geom::ShellScale;
use crate::widget::{self, Capture, Ctx};

pub use layers::Layer;
use layers::Meta;

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
    /// Shift+Tab.
    BackTab,
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
    /// A dialog's wheel goes to the hovered node; the chrome layer's to the
    /// node at `x`, `y`.
    Wheel { lines: i32, x: i32, y: i32 },
    Key(Key),
    /// Typed text for the focused `Edit` (IME commits included).
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InputResult {
    pub consumed: bool,
    /// The chrome layer's `Passthrough` node that took the input, for the
    /// host to handle (`consumed` is false then).
    pub passthrough: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    Activated { node: NodeId },
    /// A chrome node that takes the right button activated by it.
    RightActivated { node: NodeId },
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
    /// A left press in a `Modal` dialog with a `Rect` on its root, outside
    /// it: a box that closes on a click elsewhere (a pop-up card) hears
    /// it. The press reaches nothing under the box.
    PressedOutside { dialog: DialogId },
    /// A press on a `Draggable` node left it by its `DragThreshold`: the
    /// node is carried from now on (an open drop-down closed, no event).
    DragStarted { node: NodeId },
    /// The carried node moved to `x`, `y`, over the `DropTarget` `target`.
    DragMoved { node: NodeId, x: i32, y: i32, target: Option<NodeId> },
    /// The button came up with `node` carried, over `target`. The release
    /// then goes on as any other.
    Dropped { node: NodeId, target: Option<NodeId> },
    /// Tab moved the focus off `from` (none: nothing had it); `to` is the
    /// edit that took it, or none when the next stop is not an edit.
    FocusMoved { from: Option<NodeId>, to: Option<NodeId> },
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
    /// The tab stop Tab last moved to when it is no edit (the focus is then
    /// none): the next Tab goes on from it.
    tab_stop: Option<NodeId>,
    /// The node the last press landed on: Windows pairs a double-click only
    /// within one window, so a press elsewhere starts a list's sequence anew.
    last_down: Option<NodeId>,
    /// A press on a `Draggable` node, until the button comes up.
    drag: Option<Drag>,
    /// Chrome: the captured button shows pressed, as the last move (or the
    /// press) left the pointer in its rect.
    look: bool,
}

/// A press on a `Draggable` node at `from`, carried once the pointer is
/// `DragThreshold` px from it on either axis.
#[derive(Clone, Copy, Debug)]
struct Drag {
    node: NodeId,
    from: (i32, i32),
    at: (i32, i32),
    carrying: bool,
    target: Option<NodeId>,
}

/// What the host paints of a drag: the node pressed, the pointer, whether
/// it is carried yet and the `DropTarget` under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragView {
    pub node: NodeId,
    pub at: (i32, i32),
    pub carrying: bool,
    pub target: Option<NodeId>,
}

struct Dialog {
    id: DialogId,
    root: NodeId,
    kind: DialogKind,
    pointer: Pointer,
    /// Maps the dialog's device rects back to the authored base-size numbers
    /// the native widget arithmetic runs on.
    scale: ShellScale,
    /// The chrome layer under the dialogs ([`Gui::push_chrome`]).
    chrome: Option<Chrome>,
}

/// The chrome layer's hit rules.
#[derive(Clone, Copy, Debug)]
struct Chrome {
    /// No node larger than this many px² hovers.
    hover_seed_area: i64,
}

impl Dialog {
    /// The pushed look: captured and still under the pointer (612B70); in
    /// the chrome, captured with the pointer in its rect at the last move.
    fn shows_pressed(&self, id: NodeId) -> bool {
        let p = &self.pointer;
        p.pressed == Some(id)
            && match self.chrome {
                Some(_) => p.look,
                None => p.hovered == Some(id),
            }
    }
}

pub struct Gui {
    pub(crate) tree: Tree,
    /// Who made each node, its layer and root.
    pub(crate) meta: HashMap<NodeId, Meta>,
    /// Each layer's roots, bottom to top.
    pub(crate) roots: [Vec<NodeId>; 5],
    pub(crate) layout: Layout,
    revision: u64,
    /// Scratch for [`Self::remove_all`], kept for its allocation.
    gone: Vec<NodeId>,
    dialogs: Vec<Dialog>,
    next_dialog: u32,
    events: Vec<UiEvent>,
    /// List double-click limits: milliseconds within a width x height
    /// rectangle of the previous press.
    double_click: (UiMs, i32, i32),
    /// The text's length in the host's code page, for `MaxBytes`.
    byte_len: fn(&str) -> usize,
    /// The host's unscaled mapping ([`Self::set_base_scale`]).
    base_scale: ShellScale,
}

impl Gui {
    pub fn new(max_nodes: usize) -> Self {
        Gui {
            tree: Tree::new(max_nodes),
            meta: HashMap::new(),
            roots: Default::default(),
            layout: Layout::default(),
            revision: 0,
            gone: Vec::new(),
            dialogs: Vec::new(),
            next_dialog: 0,
            events: Vec::new(),
            // The host policy off Windows (`host_double_click_limits` in the
            // engine's `saved_file_input`).
            double_click: (500, 4, 4),
            byte_len: str::len,
            base_scale: ShellScale::IDENTITY,
        }
    }

    /// The host's unscaled mapping of its base layout size: the scale a
    /// dialog takes until [`Self::set_dialog_scale`], and the base a trackbar's
    /// `ScaleScreen` fits into its rect. [`ShellScale::IDENTITY`] until set.
    pub fn set_base_scale(&mut self, scale: ShellScale) {
        self.base_scale = scale;
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

    /// The host's node ([`Self::create_by`]): a root goes into
    /// [`Layer::Modal`].
    pub fn create(&mut self, kind: NodeKind, parent: Option<NodeId>) -> Result<NodeId, Refused> {
        self.create_by(kind, parent, Origin::Host)
    }

    /// A node disabled while it holds the pointer loses the capture
    /// (`EnableWindow(FALSE)`): it never paints pressed and its release
    /// activates nothing. Setting the value a prop already has changes
    /// nothing, not the revision either; the pointer rules still apply.
    /// Geometry takes effect at the next [`Self::layout`].
    pub fn set(&mut self, id: NodeId, prop: Prop, value: Value) -> Result<(), Refused> {
        let disables = prop == Prop::Enabled && value == Value::Bool(false);
        let hides = prop == Prop::Visible && value == Value::Bool(false);
        if self.tree.get(id, prop)? != Some(&value) {
            self.tree.set(id, prop, value)?;
            self.touch(id);
        }
        // The chrome's holder keeps the pointer (a gadget list's sticky
        // capture ignores the disabled byte).
        if disables {
            for d in self.dialogs.iter_mut().filter(|d| d.chrome.is_none() && d.pointer.pressed == Some(id)) {
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

    /// Pops the dialogs whose root is gone, and lets go of the removed
    /// nodes a pointer held.
    fn prune_dialogs(&mut self) {
        let tree = &self.tree;
        self.dialogs.retain(|d| tree.kind(d.root).is_ok());
        for d in &mut self.dialogs {
            let p = &mut d.pointer;
            for slot in [&mut p.pressed, &mut p.hovered, &mut p.popup, &mut p.focus, &mut p.tab_stop] {
                if slot.is_some_and(|n| tree.kind(n).is_err()) {
                    *slot = None;
                }
            }
            d.pointer.dragging &= d.pointer.pressed.is_some();
        }
    }

    /// Pushes a dialog whose root is an existing `Dialog` node, with clean
    /// pointer state. Its buttons marked `Default` and `Cancel` take Enter
    /// and Escape.
    pub fn push_dialog(&mut self, root: NodeId, kind: DialogKind) -> Result<DialogId, Refused> {
        if self.tree.kind(root)? != NodeKind::Dialog {
            return Err(Refused::WrongKind);
        }
        let id = DialogId(self.next_dialog);
        self.next_dialog += 1;
        self.dialogs.push(Dialog { id, root, kind, pointer: Pointer::default(), scale: self.base_scale, chrome: None });
        Ok(id)
    }

    /// Pushes `root`, an existing `Dialog` node, as the chrome layer: the
    /// host's own controls under every dialog. It takes the input only while
    /// no dialog is over it, and its pointer rules are a retained gadget
    /// list's ([`Self::input`]): no node larger than `hover_seed_area` px²
    /// hovers. One chrome layer at a time.
    pub fn push_chrome(&mut self, root: NodeId, hover_seed_area: i64) -> Result<DialogId, Refused> {
        if self.tree.kind(root)? != NodeKind::Dialog {
            return Err(Refused::WrongKind);
        }
        if self.chrome().is_some() {
            return Err(Refused::Denied);
        }
        let id = DialogId(self.next_dialog);
        self.next_dialog += 1;
        let chrome = Some(Chrome { hover_seed_area });
        self.dialogs.insert(0, Dialog { id, root, kind: DialogKind::Page, pointer: Pointer::default(), scale: self.base_scale, chrome });
        Ok(id)
    }

    /// The chrome layer, while it stands.
    pub fn chrome(&self) -> Option<DialogId> {
        self.dialogs.first().filter(|d| d.chrome.is_some()).map(|d| d.id)
    }

    /// The node the chrome layer would hover at `x`, `y`: its smallest live
    /// node there, none larger than its seed area.
    pub fn chrome_hit(&self, x: i32, y: i32) -> Option<NodeId> {
        let d = self.dialogs.first()?;
        self.chrome_hover_at(d.root, x, y, d.chrome?.hover_seed_area)
    }

    /// Pops the dialog and removes its nodes.
    pub fn pop_dialog(&mut self, id: DialogId) -> Result<(), Refused> {
        let pos = self.dialogs.iter().position(|d| d.id == id).ok_or(Refused::NotFound)?;
        let d = self.dialogs.remove(pos);
        self.remove(d.root)
    }

    /// Moves the dialog to the top of the stack with its state. The chrome
    /// stays under the dialogs.
    pub fn raise_dialog(&mut self, id: DialogId) -> Result<(), Refused> {
        let pos = self.dialogs.iter().position(|d| d.id == id).ok_or(Refused::NotFound)?;
        if self.dialogs[pos].chrome.is_some() {
            return Err(Refused::Denied);
        }
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
        d.pointer.drag = None;
        d.pointer.hovered = None;
        d.pointer.look = false;
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

    /// Whether the last press landed on an `Edit`. The host asks the platform's
    /// input method to show for that edit: an edit takes the keyboard only when
    /// the player taps it, not because a page gave it the focus when it opened.
    pub fn pressed_edit(&self) -> bool {
        self.top_pointer()
            .last_down
            .is_some_and(|node| self.tree.kind(node) == Ok(NodeKind::Edit))
    }

    pub fn hovered(&self) -> Option<NodeId> {
        self.top_pointer().hovered
    }

    /// The pushed look of the top dialog's `id`: captured and still under the
    /// pointer (612B70).
    pub fn shows_pressed(&self, id: NodeId) -> bool {
        self.dialogs.last().is_some_and(|d| d.shows_pressed(id))
    }

    /// [`Self::shows_pressed`] in dialog `dialog` (the chrome), on top or not.
    pub fn shows_pressed_in(&self, dialog: DialogId, id: NodeId) -> bool {
        self.dialog(dialog).is_some_and(|d| d.shows_pressed(id))
    }

    pub fn drain_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.events)
    }

    /// Routes one input to the top dialog. Down and up do not move hover:
    /// the window system sends a move before them. Events a host leaves
    /// undrained coalesce ([`append_events`]).
    pub fn input(&mut self, event: InputEvent, now: UiMs) -> InputResult {
        let start = self.events.len();
        let popup_before = self.top_pointer().popup.is_some();
        self.drag_up(&event);
        let result = self.route(event.clone(), now);
        self.drag_down_or_move(&event, popup_before);
        let new = self.events.split_off(start);
        append_events(&mut self.events, new);
        result
    }

    fn route(&mut self, event: InputEvent, now: UiMs) -> InputResult {
        let Some(top) = self.dialogs.last() else { return InputResult::default() };
        let (root, modal) = (top.root, top.kind == DialogKind::Modal);
        if let Some(chrome) = top.chrome
            && matches!(
                event,
                InputEvent::PointerDown { .. }
                    | InputEvent::PointerUp { .. }
                    | InputEvent::PointerMove { .. }
                    | InputEvent::Wheel { .. }
            )
        {
            return self.route_chrome(event, chrome.hover_seed_area);
        }
        // A left press on a hovered label (`FocusTarget`) first gives its
        // edit the keys, as `set_focus` does: no event, the caret keeps its
        // blink phase. The press then goes on as any other.
        if let InputEvent::PointerDown { button: MouseButton::Left, .. } = &event
            && let Some(edit) = self.top_pointer().hovered.and_then(|n| self.focus_target(root, n))
        {
            let _ = self.set_focus(Some(edit));
        }
        if let (InputEvent::PointerDown { x, y, button: MouseButton::Left }, Some(f)) =
            (&event, self.top_pointer().focus)
            && self.tree.node(f).is_ok_and(|n| n.bool_prop(Prop::BlurOnPress, false))
            && self.hit_at(root, *x, *y, true) != Some(f)
        {
            self.top_mut().pointer.focus = None;
        }
        if let Some(popup) = self.top_pointer().popup.filter(|n| !self.popup_hidden(*n)) {
            match event {
                InputEvent::PointerDown { x, y, button: MouseButton::Left } => {
                    self.with_ctx(now, |ctx| widget::combo::popup_press(ctx, popup, x, y));
                    return InputResult { passthrough: None, consumed: true };
                }
                InputEvent::PointerMove { x, y } => {
                    self.with_ctx(now, |ctx| widget::combo::popup_move(ctx, popup, x, y));
                    // The list is a window of its own: the dialog under it
                    // still hit-tests the rest of the screen (status help).
                    let covered = self.combo_drop_down(popup).is_some_and(|d| d.popup.rect.contains(x, y));
                    let over = self.hit(root, x, y).filter(|_| !covered);
                    self.set_hover(over);
                    return InputResult { passthrough: None, consumed: true };
                }
                InputEvent::PointerUp { x, y, button: MouseButton::Left } => {
                    self.with_ctx(now, |ctx| widget::combo::popup_release(ctx, popup, x, y));
                    return InputResult { passthrough: None, consumed: true };
                }
                // Escape closes the list (no cue) and goes no further; Enter
                // picks its hot row, as `key_popup` does, and reaches no
                // default button.
                InputEvent::Key(Key::Escape) => {
                    self.close_popup();
                    return InputResult { passthrough: None, consumed: true };
                }
                InputEvent::Key(Key::Enter) => {
                    self.with_ctx(now, |ctx| widget::combo::key(ctx, popup, Key::Enter));
                    return InputResult { passthrough: None, consumed: true };
                }
                // A `WheelPicks` list steps its selection and closes silently.
                InputEvent::Wheel { lines, .. }
                    if self.tree.node(popup).is_ok_and(|n| n.bool_prop(Prop::WheelPicks, false)) =>
                {
                    self.with_ctx(now, |ctx| widget::combo::wheel_pick(ctx, popup, lines));
                    return InputResult { passthrough: None, consumed: true };
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
                                self.with_ctx(now, |ctx| widget::trackbar::drag(ctx, held, x));
                            }
                            return InputResult { passthrough: None, consumed: true };
                        }
                        Ok(NodeKind::List) => {
                            self.with_ctx(now, |ctx| widget::list::drag(ctx, held, x, y));
                        }
                        Ok(NodeKind::Edit) => {
                            self.with_ctx(now, |ctx| widget::edit::drag(ctx, held, x, y));
                        }
                        Ok(NodeKind::ScrollBar) => {
                            // It holds the pointer: the hover stays as the
                            // press left it until the release.
                            self.with_ctx(now, |ctx| widget::scrollbar::drag(ctx, held, y));
                            return InputResult { passthrough: None, consumed: true };
                        }
                        _ => {}
                    }
                }
                let over = self.hit(root, x, y);
                self.set_hover(over);
                InputResult { passthrough: None, consumed: over.is_some() || modal }
            }
            InputEvent::PointerDown { x, y, button: MouseButton::Left } => {
                if let (true, Ok(Some(Value::Rect(r)))) = (modal, self.tree.get(root, Prop::Rect))
                    && !r.contains(x, y)
                {
                    let dialog = self.dialogs.last().map(|d| d.id).expect("a top dialog");
                    self.events.push(UiEvent::PressedOutside { dialog });
                }
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
                InputResult { passthrough: None, consumed: over.is_some() || modal }
            }
            InputEvent::PointerUp { x, y, button: MouseButton::Left } => {
                let over = self.hit_at(root, x, y, true);
                let pointer = &mut self.top_mut().pointer;
                let held = pointer.pressed.take();
                pointer.dragging = false;
                if let Some(n) = held {
                    self.release(n, x, y, over == Some(n), now);
                }
                // A `ReleaseActivates` button takes any release over it,
                // whatever the press did.
                if let Some(n) = over.filter(|n| Some(*n) != held && self.release_activates(*n)) {
                    self.activate(n);
                }
                // A `RehoverOnRelease` node let go: the hover follows the
                // pointer at once, as a move there would.
                let rehover = |n: &crate::node::Node| n.bool_prop(Prop::RehoverOnRelease, false);
                if held.is_some_and(|n| self.tree.node(n).is_ok_and(rehover)) {
                    let over = self.hit(root, x, y);
                    self.set_hover(over);
                }
                InputResult { passthrough: None, consumed: held.is_some() || over.is_some() || modal }
            }
            InputEvent::PointerDown { .. } | InputEvent::PointerUp { .. } => {
                InputResult { passthrough: None, consumed: modal }
            }
            InputEvent::Wheel { lines, .. } => {
                let target = self.top_pointer().hovered;
                let scrolled = target.is_some_and(|n| {
                    self.with_ctx(now, |ctx| match ctx.tree.kind(n) {
                        Ok(NodeKind::List) => widget::list::wheel(ctx, n, lines),
                        #[cfg(target_os = "android")]
                        Ok(NodeKind::Trackbar) => widget::trackbar::wheel(ctx, n, lines),
                        _ => false,
                    })
                });
                InputResult { passthrough: None, consumed: scrolled || modal }
            }
            InputEvent::Key(key) => {
                if let Some(f) = self.typing_focus().filter(|_| widget::is_edit_key(key)) {
                    if self.with_ctx(now, |ctx| widget::edit::key(ctx, f, key)) {
                        return InputResult { passthrough: None, consumed: true };
                    }
                }
                self.key(key, now)
            }
            InputEvent::Text(text) => match self.typing_focus() {
                Some(f) => {
                    self.with_ctx(now, |ctx| widget::edit::text(ctx, f, &text));
                    InputResult { passthrough: None, consumed: true }
                }
                None => InputResult { passthrough: None, consumed: modal },
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

    /// The focus while it takes keys and text: a hidden or disabled edit
    /// keeps the focus but types nothing (a disabled window gets no input).
    fn typing_focus(&self) -> Option<NodeId> {
        self.top_pointer()
            .focus
            .filter(|f| self.tree.effective(*f, Prop::Visible) && self.tree.effective(*f, Prop::Enabled))
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
            && self.tree.node(node).is_ok_and(|n| {
                let focused_at = match &n.state {
                    widget::WidgetState::Edit(s) => s.focused_at,
                    _ => 0,
                };
                // NewEdit's caret shows in the first of each two seconds.
                !crate::time::blink_phase(focused_at, now)
            })
    }

    /// The top dialog's focused control: its focused edit, else the control
    /// that is no edit which Tab or a press (`FocusOnPress`) last gave the
    /// focus, or [`Self::focus_control`] did.
    pub fn focused_control(&self) -> Option<NodeId> {
        let pointer = self.top_pointer();
        pointer.focus.or(pointer.tab_stop)
    }

    /// Gives the top dialog's focus to `node`, a control that is no edit,
    /// taking it off the edits (the owner's `SetFocus` on a window that reads
    /// the keys itself).
    pub fn focus_control(&mut self, node: NodeId) -> Result<(), Refused> {
        if self.tree.kind(node)? == NodeKind::Edit {
            return Err(Refused::WrongKind);
        }
        let d = self.dialogs.last_mut().ok_or(Refused::NotFound)?;
        d.pointer.focus = None;
        d.pointer.tab_stop = Some(node);
        Ok(())
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
                d.pointer.tab_stop = None;
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
        d.pointer.tab_stop = None;
        if let Some(n) = node.filter(|n| before != Some(*n)) {
            self.tree.node_mut(n)?.state = widget::WidgetState::Edit(widget::edit::EditState { focused_at: now });
        }
        Ok(())
    }

    /// Edit `node` as the NewEdit the host paints and reads: its text, caret,
    /// first shown unit (`Top`) and mask, focused when it holds the top
    /// dialog's focus.
    pub fn edit_state(&self, node: NodeId) -> Option<crate::shell::new_edit::NewEdit> {
        let n = self.tree.node(node).ok().filter(|n| n.kind == NodeKind::Edit)?;
        Some(crate::shell::new_edit::NewEdit { focused: self.focus() == Some(node), ..widget::edit::read(n) })
    }

    /// NewEdit keeps Edit `node`'s caret in view (`scroll_to_caret`): its
    /// `Top` becomes the first unit from which the text up to the caret fits
    /// the field. `measure` is the host font's text width in base px; the
    /// field is the node's rect at its dialog's scale less NewEdit's 3 px,
    /// or for a framed edit (`TextInset`) less the inset and the caret's 2.
    /// Running it again with nothing changed changes nothing, so a host may
    /// run it every frame.
    pub fn keep_caret_in_view(&mut self, node: NodeId, measure: &dyn Fn(&str) -> u32) -> Result<(), Refused> {
        if self.tree.kind(node)? != NodeKind::Edit {
            return Err(Refused::WrongKind);
        }
        let scale = self.scale_of(node)?;
        widget::edit::keep_caret_in_view(&mut self.tree, node, scale, measure);
        Ok(())
    }

    /// Writes a NewEdit's text, caret and first shown unit to Edit `node`, as
    /// a host `set` (no event).
    pub fn set_edit_state(&mut self, node: NodeId, edit: &crate::shell::new_edit::NewEdit) -> Result<(), Refused> {
        if self.tree.kind(node)? != NodeKind::Edit {
            return Err(Refused::WrongKind);
        }
        widget::edit::write(&mut self.tree, node, edit);
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

    /// The wheel for List `node` of the top dialog wherever the pointer is,
    /// for a page whose one list takes every notch; true when it scrolled (`Scrolled`). A hidden list takes none.
    /// The list scrolls at its own dialog's scale, on top or not.
    pub fn wheel_list(&mut self, node: NodeId, lines: i32, now: UiMs) -> bool {
        if self.tree.kind(node) != Ok(NodeKind::List) || !self.tree.effective(node, Prop::Visible) {
            return false;
        }
        let Some(at) = self.dialog_index_of(node) else { return false };
        self.with_ctx_in(at, now, |ctx| widget::list::wheel(ctx, node, lines))
    }

    /// The wheel for the top dialog's open drop-down, for a host whose lists
    /// take it: `rows` on when the pointer at `x`, `y` is
    /// over the list. True when a list is open, which then owns the wheel.
    pub fn wheel_popup(&mut self, rows: i32, x: i32, y: i32, now: UiMs) -> bool {
        let Some(node) = self.popup().filter(|n| !self.popup_hidden(*n)) else { return false };
        if self.combo_drop_down(node).is_some_and(|d| d.popup.rect.contains(x, y)) {
            self.with_ctx(now, |ctx| widget::combo::wheel(ctx, node, rows, x, y));
        }
        true
    }

    /// A key for the top dialog's open drop-down, for a host whose lists
    /// take the keyboard: Up/Down move its hot row,
    /// Enter picks it, Escape closes. True when a list is open, which then
    /// owns the keys.
    pub fn key_popup(&mut self, key: Key, now: UiMs) -> bool {
        let Some(node) = self.popup().filter(|n| !self.popup_hidden(*n)) else { return false };
        self.with_ctx(now, |ctx| widget::combo::key(ctx, node, key));
        true
    }

    /// The `Edit` under `root` whose keys label `node` gives (its
    /// `FocusTarget` host id).
    fn focus_target(&self, root: NodeId, node: NodeId) -> Option<NodeId> {
        let Ok(Some(Value::Int(id))) = self.tree.get(node, Prop::FocusTarget) else { return None };
        self.find_id(root, *id).filter(|n| self.tree.kind(*n) == Ok(NodeKind::Edit))
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

    /// A `MultiSelect` List's selected rows, in the order they were selected.
    pub fn selected_rows(&self, node: NodeId) -> Vec<usize> {
        self.tree.node(node).map_or_else(|_| Vec::new(), widget::list::selected_rows)
    }

    /// Selects `rows` of the `MultiSelect` List `node` (the host refilled it).
    pub fn set_selected_rows(&mut self, node: NodeId, rows: &[usize]) -> Result<(), Refused> {
        let rows = rows.iter().map(usize::to_string).collect();
        self.tree.set(node, Prop::SelectedRows, Value::Items(rows))
    }

    /// The top dialog's drag, from the press on a `Draggable` node until
    /// the button comes up.
    pub fn drag(&self) -> Option<DragView> {
        let d = self.dialogs.last()?.pointer.drag?;
        Some(DragView { node: d.node, at: d.at, carrying: d.carrying, target: d.target })
    }

    /// The release: a carried node is dropped (before the release's own
    /// events); any drag ends.
    fn drag_up(&mut self, event: &InputEvent) {
        if !matches!(event, InputEvent::PointerUp { button: MouseButton::Left, .. }) {
            return;
        }
        let Some(d) = self.dialogs.last_mut() else { return };
        if let Some(drag) = d.pointer.drag.take().filter(|drag| drag.carrying) {
            self.events.push(UiEvent::Dropped { node: drag.node, target: drag.target });
        }
    }

    /// A press on a `Draggable` node (one no open drop-down took) starts a
    /// drag; a move carries it once past the node's `DragThreshold` and
    /// reports the `DropTarget` under the pointer.
    fn drag_down_or_move(&mut self, event: &InputEvent, popup_before: bool) {
        let Some(top) = self.dialogs.last() else { return };
        let root = top.root;
        match *event {
            InputEvent::PointerDown { x, y, button: MouseButton::Left } if !popup_before => {
                let node = self.hit_at(root, x, y, true).filter(|n| {
                    self.tree.effective(*n, Prop::Enabled)
                        && matches!(self.tree.get(*n, Prop::Draggable), Ok(Some(Value::Bool(true))))
                });
                self.top_mut().pointer.drag = node.map(|node| Drag {
                    node,
                    from: (x, y),
                    at: (x, y),
                    carrying: false,
                    target: None,
                });
            }
            InputEvent::PointerMove { x, y } => {
                let Some(mut drag) = top.pointer.drag else { return };
                drag.at = (x, y);
                if !drag.carrying {
                    let threshold = match self.tree.get(drag.node, Prop::DragThreshold) {
                        Ok(Some(Value::Int(px))) => *px,
                        _ => 0,
                    };
                    let (dx, dy) = ((x - drag.from.0).abs(), (y - drag.from.1).abs());
                    if dx.max(dy) >= threshold {
                        drag.carrying = true;
                        self.close_popup();
                        self.events.push(UiEvent::DragStarted { node: drag.node });
                    }
                }
                if drag.carrying {
                    drag.target = self.drop_target_at(root, x, y);
                    let (node, target) = (drag.node, drag.target);
                    self.events.push(UiEvent::DragMoved { node, x, y, target });
                }
                self.top_mut().pointer.drag = Some(drag);
            }
            _ => {}
        }
    }

    /// The first visible `DropTarget` under `root` at `x`, `y`.
    fn drop_target_at(&self, root: NodeId, x: i32, y: i32) -> Option<NodeId> {
        self.tree
            .walk(root, &mut |id, _| {
                if matches!(self.tree.get(id, Prop::DropTarget), Ok(Some(Value::Bool(true))))
                    && self.tree.effective(id, Prop::Visible)
                    && self.tree.screen_hit_rect(id).is_ok_and(|r| r.contains(x, y))
                {
                    return ControlFlow::Break(id);
                }
                ControlFlow::Continue(())
            })
            .break_value()
    }

    /// What the host paints of List `node`.
    pub fn list_view(&self, node: NodeId) -> Option<widget::list::ListView> {
        widget::list::view(&self.tree, node)
    }

    /// What the host paints of ScrollBar `node`, at its own dialog's scale.
    pub fn scrollbar_view(&self, node: NodeId) -> Option<widget::scrollbar::ScrollBarView> {
        if self.tree.kind(node) != Ok(NodeKind::ScrollBar) {
            return None;
        }
        let held = self.dialogs.iter().any(|d| d.pointer.pressed == Some(node));
        widget::scrollbar::view(&self.tree, node, self.scale_of(node).ok()?, held)
    }

    /// Trackbar `node`'s range as it hits: `Min`, `Max`, `Step` and, by
    /// `ShowValue`, its value plaque's reserve.
    pub fn trackbar_range(&self, node: NodeId) -> Option<crate::shell::trackbar::TrackbarRange> {
        let n = self.tree.node(node).ok().filter(|n| n.kind == NodeKind::Trackbar)?;
        Some(widget::trackbar::range(n))
    }

    /// List `node` as a transcript (a chat log) now holds `rows` rows, of
    /// which `dropped` left its front: a view at its end, or never
    /// scrolled, follows the new last row, any other keeps showing the same
    /// rows. The list owns the top; the host only reports the rows.
    pub fn log_rows(&mut self, node: NodeId, rows: usize, dropped: usize) -> Result<(), Refused> {
        if self.tree.kind(node)? != NodeKind::List {
            return Err(Refused::WrongKind);
        }
        let scale = self.scale_of(node)?;
        widget::list::log_rows(&mut self.tree, node, scale, rows, dropped);
        Ok(())
    }

    /// Scrolls List `node` the least that shows `row` whole, at its dialog's
    /// scale: above the top it becomes the top, below the last shown row
    /// the last; the top stays within the range. No event (a host change).
    pub fn ensure_row_visible(&mut self, node: NodeId, row: usize) -> Result<(), Refused> {
        if self.tree.kind(node)? != NodeKind::List {
            return Err(Refused::WrongKind);
        }
        let scale = self.scale_of(node)?;
        widget::list::ensure_row_visible(&mut self.tree, node, scale, row);
        Ok(())
    }

    /// List `node` as it shows and hits now: its rows, top and scrollbar on
    /// its rect, at its dialog's scale (the row under a point, its range).
    pub fn list_geometry(&self, node: NodeId) -> Option<crate::shell::list::ShellListGeometry> {
        let scale = self.scale_of(node).ok()?;
        widget::list::geometry(&self.tree, node, scale)
    }

    /// The row of List `node` under `(x, y)`, as a press there would find
    /// it, whether or not the list takes presses now.
    pub fn list_row_at(&self, node: NodeId, x: i32, y: i32) -> Option<usize> {
        let view = self.list_view(node)?;
        self.list_geometry(node)?.row_at(view.rows, view.top, x, y)
    }

    /// The scale of the dialog `node` lies in, on top or not.
    fn scale_of(&self, node: NodeId) -> Result<ShellScale, Refused> {
        self.tree.kind(node)?;
        Ok(self.dialog_index_of(node).map_or(self.base_scale, |at| self.dialogs[at].scale))
    }

    /// The stack index of the dialog `node` lies in.
    fn dialog_index_of(&self, node: NodeId) -> Option<usize> {
        let mut root = node;
        while let Ok(Some(parent)) = self.tree.parent(root) {
            root = parent;
        }
        self.dialogs.iter().position(|d| d.root == root)
    }

    fn with_ctx<R>(&mut self, now: UiMs, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let top = self.dialogs.len().checked_sub(1).expect("callers check for a top dialog");
        self.with_ctx_in(top, now, f)
    }

    /// The widgets' view of the dialog at stack index `at`.
    fn with_ctx_in<R>(&mut self, at: usize, now: UiMs, f: impl FnOnce(&mut Ctx<'_>) -> R) -> R {
        let d = &mut self.dialogs[at];
        let mut ctx = Ctx {
            tree: &mut self.tree,
            events: &mut self.events,
            scale: d.scale,
            base_scale: self.base_scale,
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
        if n.bool_prop(Prop::FocusOnPress, false) {
            let pointer = &mut self.top_mut().pointer;
            pointer.focus = None;
            pointer.tab_stop = Some(node);
        }
        let Ok(n) = self.tree.node(node) else { return };
        if n.kind == NodeKind::Text {
            return;
        }
        let delegated = match n.kind {
            NodeKind::List => Some(self.with_ctx(now, |ctx| widget::list::press(ctx, node, x, y))),
            NodeKind::Combo => Some(self.with_ctx(now, |ctx| widget::combo::press(ctx, node, x, y))),
            NodeKind::Edit => Some(self.with_ctx(now, |ctx| widget::edit::press(ctx, node, x, y))),
            NodeKind::ScrollBar => Some(self.with_ctx(now, |ctx| widget::scrollbar::press(ctx, node, x, y))),
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
            let scale = self.dialogs.last().map_or(self.base_scale, |d| d.scale);
            let anywhere = n.props.contains_key(&Prop::HitRect);
            if anywhere || scale.derive_rect(rect, |r| Rect::new(r.x, r.y, 18, 18)).contains(x, y) {
                self.toggle(node);
            }
            return;
        }
        if n.kind == NodeKind::Trackbar {
            // Any press on the window holds it; only one on the thumb drags.
            if let Some(drag) = self.with_ctx(now, |ctx| widget::trackbar::press(ctx, node, x, y)) {
                let pointer = &mut self.top_mut().pointer;
                pointer.pressed = Some(node);
                pointer.dragging |= drag;
            }
            return;
        }
        self.top_mut().pointer.pressed = Some(node);
        self.events.push(UiEvent::Pressed { node });
    }

    /// The release of a captured node; `over` when the pointer is still on it.
    fn release(&mut self, node: NodeId, x: i32, y: i32, over: bool, now: UiMs) {
        let Ok(n) = self.tree.node(node) else { return };
        let enabled = self.tree.effective(node, Prop::Enabled);
        match n.kind {
            NodeKind::List => self.with_ctx(now, |ctx| widget::list::release(ctx, node, x, y)),
            NodeKind::Combo => self.with_ctx(now, |ctx| widget::combo::release(ctx, node, x, y)),
            NodeKind::Edit => self.with_ctx(now, |ctx| widget::edit::release(ctx, node, x, y)),
            NodeKind::ScrollBar => self.with_ctx(now, |ctx| widget::scrollbar::release(ctx, node)),
            NodeKind::Trackbar => self.with_ctx(now, |ctx| widget::trackbar::release(ctx, node)),
            _ if over && enabled => self.activate(node),
            _ => {}
        }
    }

    fn toggle(&mut self, node: NodeId) {
        let checked = !self.tree.node(node).is_ok_and(|n| n.bool_prop(Prop::Checked, false));
        let _ = self.tree.set(node, Prop::Checked, Value::Bool(checked));
        self.events.push(UiEvent::Changed { node, value: Value::Bool(checked), final_: true });
    }

    fn top_mut(&mut self) -> &mut Dialog {
        self.dialogs.last_mut().expect("callers check for a top dialog")
    }

    /// The chrome layer's pointer input: a retained gadget list's rules.
    ///
    /// - Hover: the smallest enabled node under the pointer, the later one
    ///   on a tie, none larger than the layer's seed area.
    /// - A press goes to the first enabled node in tree order under it that
    ///   takes the button (`Buttons`): a `PressActivates` node activates at
    ///   once, any other takes the capture, a `Passthrough` one for the host.
    /// - The holder takes every button message, enabled or not, until the
    ///   release of a button it takes; a release with no holder reaches only
    ///   a catcher. An `Opaque` node swallows what it does not act on.
    /// - A held button shows pressed while the last move (or the press) left
    ///   the pointer in its rect, and its release activates it only then: a
    ///   release off it with no move since still activates it, but toggles
    ///   it only with the pointer inside.
    fn route_chrome(&mut self, event: InputEvent, seed: i64) -> InputResult {
        let root = self.top_mut().root;
        match event {
            InputEvent::PointerMove { x, y } => {
                let over = self.chrome_hover_at(root, x, y, seed);
                self.set_hover(over);
                if let Some(held) = self.top_pointer().pressed {
                    let inside = self.inside(held, x, y);
                    self.top_mut().pointer.look = inside;
                }
                let consumed = over.is_some_and(|n| !self.chrome_flag(n, Prop::Passthrough));
                InputResult { passthrough: None, consumed }
            }
            InputEvent::PointerDown { x, y, button } => {
                let bit = button_bit(button);
                if let Some(held) = self.top_pointer().pressed {
                    // A press of a button the holder takes presses it again;
                    // any other only follows the pointer.
                    let look = self.takes(held, bit) || self.inside(held, x, y);
                    self.top_mut().pointer.look = look;
                    return self.held_result(held);
                }
                let at = |g: &Self, n| g.takes(n, bit) || g.chrome_flag(n, Prop::Opaque);
                let Some(node) = self.chrome_at(root, x, y, at) else {
                    return InputResult::default();
                };
                if !self.takes(node, bit) {
                    // An `Opaque` node swallows a button it does not take.
                    return InputResult { passthrough: None, consumed: true };
                }
                if self.chrome_flag(node, Prop::PressActivates) {
                    self.activate_by(node, button);
                    return InputResult { passthrough: None, consumed: true };
                }
                let pointer = &mut self.top_mut().pointer;
                pointer.pressed = Some(node);
                pointer.look = true;
                if self.chrome_flag(node, Prop::Passthrough) {
                    return InputResult { passthrough: Some(node), consumed: false };
                }
                self.events.push(UiEvent::Pressed { node });
                InputResult { passthrough: None, consumed: true }
            }
            InputEvent::PointerUp { x, y, button } => {
                let bit = button_bit(button);
                let Some(held) = self.top_pointer().pressed else {
                    let at = |g: &Self, n| {
                        g.takes(n, bit) && g.chrome_flag(n, Prop::Passthrough) || g.chrome_flag(n, Prop::Opaque)
                    };
                    return match self.chrome_at(root, x, y, at) {
                        Some(n) if self.chrome_flag(n, Prop::Passthrough) => {
                            InputResult { passthrough: Some(n), consumed: false }
                        }
                        Some(_) => InputResult { passthrough: None, consumed: true },
                        None => InputResult::default(),
                    };
                };
                let result = self.held_result(held);
                if !self.takes(held, bit) {
                    let inside = self.inside(held, x, y);
                    self.top_mut().pointer.look = inside;
                    return result;
                }
                let pointer = &mut self.top_mut().pointer;
                let look = std::mem::take(&mut pointer.look);
                pointer.pressed = None;
                if look && !self.chrome_flag(held, Prop::Passthrough) {
                    if self.inside(held, x, y) {
                        self.toggle_on_release(held);
                    }
                    self.activate_by(held, button);
                }
                result
            }
            InputEvent::Wheel { x, y, .. } => {
                let node = self.chrome_at(root, x, y, |_, _| true);
                match node.filter(|n| self.chrome_flag(*n, Prop::Passthrough)) {
                    Some(n) => InputResult { passthrough: Some(n), consumed: false },
                    None => InputResult { passthrough: None, consumed: node.is_some() },
                }
            }
            InputEvent::Key(_) | InputEvent::Text(_) => InputResult::default(),
        }
    }

    /// What the chrome's holder reports for an input it takes.
    fn held_result(&self, held: NodeId) -> InputResult {
        match self.chrome_flag(held, Prop::Passthrough) {
            true => InputResult { passthrough: Some(held), consumed: false },
            false => InputResult { passthrough: None, consumed: true },
        }
    }

    fn chrome_flag(&self, node: NodeId, prop: Prop) -> bool {
        self.tree.node(node).is_ok_and(|n| n.bool_prop(prop, false))
    }

    /// Whether chrome `node` takes the mouse button `bit` (`Buttons`).
    fn takes(&self, node: NodeId, bit: i32) -> bool {
        self.tree.node(node).is_ok_and(|n| n.int_prop(Prop::Buttons, 1) & bit != 0)
    }

    fn inside(&self, node: NodeId, x: i32, y: i32) -> bool {
        self.tree.screen_hit_rect(node).is_ok_and(|r| r.contains(x, y))
    }

    /// A chrome node that can take the pointer: a shown, enabled button.
    fn chrome_live(&self, node: NodeId, kind: NodeKind) -> bool {
        kind == NodeKind::Button && self.tree.effective(node, Prop::Visible) && self.tree.effective(node, Prop::Enabled)
    }

    /// The first live chrome node under `root` in tree order at `x`, `y`
    /// that `accept` takes.
    fn chrome_at(&self, root: NodeId, x: i32, y: i32, accept: impl Fn(&Self, NodeId) -> bool) -> Option<NodeId> {
        self.tree
            .walk(root, &mut |id, kind| {
                match self.chrome_live(id, kind) && self.inside(id, x, y) && accept(self, id) {
                    true => ControlFlow::Break(id),
                    false => ControlFlow::Continue(()),
                }
            })
            .break_value()
    }

    /// The chrome's hover at `x`, `y`: the smallest live node there, the
    /// later on a tie (a signed `<=`), seeded with `seed` so that none larger
    /// wins.
    fn chrome_hover_at(&self, root: NodeId, x: i32, y: i32, seed: i64) -> Option<NodeId> {
        let mut best = None;
        let mut best_area = seed;
        let _ = self.tree.walk(root, &mut |id, kind| {
            if self.chrome_live(id, kind)
                && let Ok(r) = self.tree.screen_hit_rect(id)
                && r.contains(x, y)
            {
                let area = i64::from(r.w) * i64::from(r.h);
                if area <= best_area {
                    best = Some(id);
                    best_area = area;
                }
            }
            ControlFlow::<()>::Continue(())
        });
        best
    }

    fn activate_by(&mut self, node: NodeId, button: MouseButton) {
        self.events.push(match button {
            MouseButton::Left => UiEvent::Activated { node },
            MouseButton::Right => UiEvent::RightActivated { node },
        });
    }

    /// A `ToggleOnRelease` button let go inside: `Toggled` flips (1) or
    /// turns on (2).
    fn toggle_on_release(&mut self, node: NodeId) {
        let Ok(n) = self.tree.node(node) else { return };
        let on = match n.int_prop(Prop::ToggleOnRelease, 0) {
            1 => !n.bool_prop(Prop::Toggled, false),
            2 => true,
            _ => return,
        };
        let _ = self.tree.set(node, Prop::Toggled, Value::Bool(on));
    }

    fn key(&mut self, key: Key, now: UiMs) -> InputResult {
        let Some(top) = self.dialogs.last() else { return InputResult::default() };
        let (root, modal) = (top.root, top.kind == DialogKind::Modal);
        let marked = match key {
            Key::Enter => Prop::Default,
            Key::Escape => Prop::Cancel,
            Key::Tab | Key::BackTab => {
                return InputResult { passthrough: None, consumed: self.tab(root, key == Key::BackTab, now) || modal };
            }
            _ => return InputResult { passthrough: None, consumed: modal },
        };
        let target = self
            .tree
            .walk(root, &mut |id, kind| {
                let on = matches!(self.tree.get(id, marked), Ok(Some(Value::Bool(true))));
                match kind == NodeKind::Button && on {
                    true => ControlFlow::Break(id),
                    false => ControlFlow::Continue(()),
                }
            })
            .break_value();
        match target {
            Some(n) if self.tree.effective(n, Prop::Enabled) => {
                self.activate(n);
                InputResult { passthrough: None, consumed: true }
            }
            _ => InputResult { passthrough: None, consumed: modal },
        }
    }

    /// Tab from the focused edit, or the stop Tab last reached, to the next
    /// tab stop of the dialog in tree order (`back`: the previous), wrapping,
    /// among the shown and enabled ones (GetNextDlgTabItem). A stop that is no
    /// edit takes the focus off the edits and is where the next Tab starts.
    /// Without either, Tab either way goes to the first stop. An edit with
    /// `KeepsBlink` takes the focus without restarting its caret blink.
    fn tab(&mut self, root: NodeId, back: bool, now: UiMs) -> bool {
        let pointer = self.top_pointer();
        let from = pointer.focus;
        let mut stops = Vec::new();
        let _ = self.tree.walk(root, &mut |id, kind| {
            let stop = match self.tree.get(id, Prop::TabStop) {
                Ok(Some(Value::Bool(on))) => *on,
                _ => kind == NodeKind::Edit,
            };
            if stop && kind.is_focusable() && self.tree.effective(id, Prop::Visible) && self.tree.effective(id, Prop::Enabled)
            {
                stops.push(id);
            }
            ControlFlow::<()>::Continue(())
        });
        let current = from.or(pointer.tab_stop);
        let next = match current.and_then(|c| stops.iter().position(|n| *n == c)) {
            Some(at) => stops[if back { (at + stops.len() - 1) % stops.len() } else { (at + 1) % stops.len() }],
            // A focused edit that is no stop keeps the focus.
            None if from.is_some() => return false,
            None => match stops.first() {
                Some(first) => *first,
                None => return false,
            },
        };
        if Some(next) != current {
            let to = (self.tree.kind(next) == Ok(NodeKind::Edit)).then_some(next);
            if to.is_some_and(|n| matches!(self.tree.get(n, Prop::KeepsBlink), Ok(Some(Value::Bool(true))))) {
                self.top_mut().pointer.focus = to;
            } else {
                let top = self.top_mut().id;
                let _ = self.focus_edit_in(top, to, now);
            }
            self.top_mut().pointer.tab_stop = if to.is_none() { Some(next) } else { None };
            self.events.push(UiEvent::FocusMoved { from, to });
        }
        true
    }

    fn release_activates(&self, node: NodeId) -> bool {
        matches!(self.tree.get(node, Prop::ReleaseActivates), Ok(Some(Value::Bool(true))))
            && self.tree.effective(node, Prop::Enabled)
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
        self.tree
            .walk(root, &mut |id, kind| {
                // A static or picture with status help hovers too (it takes
                // no press).
                let help = matches!(kind, NodeKind::Text | NodeKind::Image)
                    && self.tree.get(id, Prop::Tooltip).is_ok_and(|t| t.is_some());
                let through = press && matches!(self.tree.get(id, Prop::PressThrough), Ok(Some(Value::Bool(true))));
                if !through
                    && (kind.is_focusable() || help)
                    && self.tree.effective(id, Prop::Visible)
                    && self.tree.screen_hit_rect(id).is_ok_and(|r| r.contains(x, y))
                {
                    return ControlFlow::Break(id);
                }
                ControlFlow::Continue(())
            })
            .break_value()
    }
}

/// A mouse button as the chrome's `Buttons` bit.
fn button_bit(button: MouseButton) -> i32 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
    }
}

/// Appends `new` to undrained `events`, coalescing what only the latest of
/// matters for: a hover that ends while its start is still undrained cancels
/// it, and a node's newer `Scrolled` replaces its undrained one right before
/// (the host reads the top from the node). Nothing else merges.
pub fn append_events(events: &mut Vec<UiEvent>, new: impl IntoIterator<Item = UiEvent>) {
    for event in new {
        match (&event, events.last()) {
            (UiEvent::Hovered { node, entered: false }, Some(UiEvent::Hovered { node: last, entered: true }))
                if node == last =>
            {
                events.pop();
            }
            (UiEvent::Scrolled { node, .. }, Some(UiEvent::Scrolled { node: last, .. })) if node == last => {
                events.pop();
                events.push(event);
            }
            _ => events.push(event),
        }
    }
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
        g.push_dialog(root, DialogKind::Page).unwrap();
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
    fn a_press_on_an_edit_is_reported_for_the_hosts_input_method() {
        let mut g = Gui::new(16);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let edit = g.create(NodeKind::Edit, Some(root)).unwrap();
        let button = g.create(NodeKind::Button, Some(root)).unwrap();
        g.set(edit, Prop::Rect, Value::Rect(Rect::new(0, 0, 40, 10))).unwrap();
        g.set(button, Prop::Rect, Value::Rect(Rect::new(0, 10, 40, 10))).unwrap();
        g.push_dialog(root, DialogKind::Page).unwrap();
        assert!(!g.pressed_edit(), "nothing was pressed yet");
        down(&mut g, 5);
        assert!(g.pressed_edit(), "the press landed on the edit");
        assert_eq!(g.focus(), Some(edit), "and gave it the keyboard focus");
        g.input(InputEvent::PointerMove { x: 5, y: 15 }, 0);
        g.input(InputEvent::PointerDown { x: 5, y: 15, button: MouseButton::Left }, 0);
        assert!(!g.pressed_edit(), "a press on a button is not an edit press");
    }

    #[test]
    fn a_rehover_on_release_node_lets_the_hover_follow_its_release() {
        let (mut g, a, b) = page_with_two_buttons();
        let release_at_b = |g: &mut Gui| {
            down(g, 5);
            g.input(InputEvent::PointerUp { x: 15, y: 5, button: MouseButton::Left }, 0);
            g.hovered()
        };
        assert_eq!(release_at_b(&mut g), Some(a), "the hover waits for a move");
        g.set(a, Prop::RehoverOnRelease, Value::Bool(true)).unwrap();
        assert_eq!(release_at_b(&mut g), Some(b));
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
        g.set(ok, Prop::Default, Value::Bool(true)).unwrap();
        let id = g.push_dialog(m, DialogKind::Modal).unwrap();
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
        g.push_dialog(root, DialogKind::Page).unwrap();
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
        g.set(ok, Prop::Default, Value::Bool(true)).unwrap();
        g.set(cancel, Prop::Cancel, Value::Bool(true)).unwrap();
        g.push_dialog(root, DialogKind::Page).unwrap();
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

    /// A press on a `Draggable` node is carried only once the pointer is
    /// its `DragThreshold` away, then reports the `DropTarget` under it and
    /// drops there on the release; a short press is no drag.
    #[test]
    fn a_draggable_press_is_carried_past_its_threshold_and_dropped_on_a_target() {
        let (mut g, a, b) = page_with_two_buttons();
        g.set(a, Prop::Draggable, Value::Bool(true)).unwrap();
        g.set(a, Prop::DragThreshold, Value::Int(6)).unwrap();
        g.set(b, Prop::DropTarget, Value::Bool(true)).unwrap();
        let drags = |g: &mut Gui| -> Vec<UiEvent> {
            g.drain_events()
                .into_iter()
                .filter(|e| matches!(e, UiEvent::DragStarted { .. } | UiEvent::DragMoved { .. } | UiEvent::Dropped { .. }))
                .collect()
        };
        let (ax, ay) = { let r = g.tree().screen_rect(a).unwrap(); (r.x + 1, r.y + 1) };
        let (bx, by) = { let r = g.tree().screen_rect(b).unwrap(); (r.x + 1, r.y + 1) };
        g.input(InputEvent::PointerDown { x: ax, y: ay, button: MouseButton::Left }, 0);
        g.input(InputEvent::PointerMove { x: ax + 5, y: ay }, 0);
        assert_eq!(drags(&mut g), vec![]);
        assert!(g.drag().is_some_and(|d| !d.carrying));
        g.input(InputEvent::PointerMove { x: bx, y: by }, 0);
        assert_eq!(
            drags(&mut g),
            vec![UiEvent::DragStarted { node: a }, UiEvent::DragMoved { node: a, x: bx, y: by, target: Some(b) }]
        );
        g.input(InputEvent::PointerUp { x: bx, y: by, button: MouseButton::Left }, 0);
        assert_eq!(drags(&mut g), vec![UiEvent::Dropped { node: a, target: Some(b) }]);
        assert_eq!(g.drag(), None);
        // A short press and release drops nothing.
        g.input(InputEvent::PointerDown { x: ax, y: ay, button: MouseButton::Left }, 0);
        g.input(InputEvent::PointerUp { x: ax, y: ay, button: MouseButton::Left }, 0);
        assert_eq!(drags(&mut g), vec![]);
    }

    /// A modal box with a `Rect` hears a press outside it; one inside, or a
    /// box without a rect, does not.
    #[test]
    fn a_press_outside_a_modal_box_is_reported() {
        let mut g = Gui::new(8);
        let root = g.create(NodeKind::Dialog, None).unwrap();
        let dialog = g.push_dialog(root, DialogKind::Modal).unwrap();
        let down = |x, y| InputEvent::PointerDown { x, y, button: MouseButton::Left };
        g.input(down(5, 5), 0);
        assert_eq!(g.drain_events(), vec![]);
        g.set(root, Prop::Rect, Value::Rect(Rect::new(10, 10, 100, 50))).unwrap();
        g.input(down(20, 20), 0);
        assert_eq!(g.drain_events(), vec![]);
        g.input(down(5, 5), 0);
        assert_eq!(g.drain_events(), vec![UiEvent::PressedOutside { dialog }]);
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

    #[test]
    fn multiline_enter_appends_crlf_and_clicks_the_default_only_for_a_trimmed_text() {
        let (mut g, edit, ok, _) = page_with_edit();
        g.set(edit, Prop::Multiline, Value::Bool(true)).unwrap();
        g.set(edit, Prop::MaxLen, Value::Int(3)).unwrap();
        g.set_focus(Some(edit)).unwrap();
        // An empty text gains CRLF but clicks nothing.
        assert!(g.input(InputEvent::Key(Key::Enter), 0).consumed);
        assert_eq!(text_of(&g, edit), ("\r\n".into(), 0), "the caret stays where it was");
        assert!(!g.drain_events().iter().any(|e| matches!(e, UiEvent::Activated { .. })));
        // Typed at the kept caret, then past MaxLen on Enter.
        g.input(InputEvent::Text("a".into()), 0);
        g.drain_events();
        g.input(InputEvent::Key(Key::Enter), 0);
        assert_eq!(text_of(&g, edit), ("a\r\n\r\n".into(), 1));
        let events = g.drain_events();
        assert_eq!(events.last(), Some(&UiEvent::Activated { node: ok }));
        assert!(events.contains(&UiEvent::Submitted { node: edit, text: "a\r\n\r\n".into() }));
    }

    #[test]
    fn keep_caret_in_view_scrolls_the_field_less_newedit_or_frame_insets() {
        let (mut g, edit, _, _) = page_with_edit();
        // Ten px a unit; the 100 px rect shows 97 px, nine units.
        let measure = |text: &str| text.encode_utf16().count() as u32 * 10;
        g.set(edit, Prop::Text, Value::Text("abcdefghijkl".into())).unwrap();
        g.keep_caret_in_view(edit, &measure).unwrap();
        assert_eq!(g.edit_state(edit).unwrap().first_visible_unit, 3);
        // Framed, 3 + 1 + 2 px less: a 95 px rect shows 89 px, eight units.
        g.set(edit, Prop::TextInset, Value::Rect(Rect::new(3, 1, 1, 1))).unwrap();
        g.set(edit, Prop::Rect, Value::Rect(Rect::new(0, 0, 95, 10))).unwrap();
        g.keep_caret_in_view(edit, &measure).unwrap();
        assert_eq!(g.edit_state(edit).unwrap().first_visible_unit, 4);
        // The scroll only follows the caret back, never ahead of it.
        g.set(edit, Prop::Caret, Value::Int(2)).unwrap();
        g.keep_caret_in_view(edit, &measure).unwrap();
        assert_eq!(g.edit_state(edit).unwrap().first_visible_unit, 2);
    }
    #[test]
    fn hidden_default_and_cancel_buttons_take_enter_and_escape_unless_disabled() {
        let (mut g, _, ok, cancel) = page_with_edit();
        g.set(ok, Prop::Visible, Value::Bool(false)).unwrap();
        g.set(cancel, Prop::Enabled, Value::Bool(false)).unwrap();
        assert!(g.input(InputEvent::Key(Key::Enter), 0).consumed, "a hidden IDOK still answers");
        assert!(!g.input(InputEvent::Key(Key::Escape), 0).consumed);
        assert_eq!(g.drain_events(), [UiEvent::Activated { node: ok }]);
    }

    #[test]
    fn tab_walks_the_shown_enabled_stops_in_tree_order_and_a_button_stop_blurs() {
        let (mut g, a, ok, _) = page_with_edit();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let hidden = g.create(NodeKind::Edit, Some(root)).unwrap();
        let b = g.create(NodeKind::Edit, Some(root)).unwrap();
        g.set(hidden, Prop::Visible, Value::Bool(false)).unwrap();
        g.input(InputEvent::Key(Key::BackTab), 0);
        assert_eq!(g.focus(), Some(a), "without a focus, the first stop either way");
        g.input(InputEvent::Key(Key::Tab), 0);
        assert_eq!(g.focus(), Some(b));
        g.input(InputEvent::Key(Key::Tab), 0);
        assert_eq!(g.focus(), Some(a), "wraps past the hidden edit");
        g.input(InputEvent::Key(Key::BackTab), 0);
        assert_eq!(g.focus(), Some(b));
        g.set(ok, Prop::TabStop, Value::Bool(true)).unwrap();
        g.focus_edit(Some(a), 0).unwrap();
        g.drain_events();
        assert!(g.input(InputEvent::Key(Key::Tab), 0).consumed);
        assert_eq!(g.focus(), None, "the button stop takes the focus off the edits");
        assert_eq!(g.drain_events(), [UiEvent::FocusMoved { from: Some(a), to: None }]);
    }
    #[test]
    fn escape_closes_an_open_list_before_it_reaches_the_cancel_button() {
        let (mut g, _, _, cancel) = page_with_edit();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let combo = g.create(NodeKind::Combo, Some(root)).unwrap();
        g.set(combo, Prop::Items, Value::Items(vec!["a".into()])).unwrap();
        g.open_popup(combo, 0).unwrap();
        assert!(g.input(InputEvent::Key(Key::Escape), 0).consumed);
        assert_eq!((g.popup(), g.drain_events()), (None, vec![]));
        g.input(InputEvent::Key(Key::Escape), 0);
        assert_eq!(g.drain_events(), [UiEvent::Activated { node: cancel }]);
    }

    #[test]
    fn a_keeps_blink_edit_takes_tab_focus_in_its_old_blink_phase() {
        let (mut g, a, _, _) = page_with_edit();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let b = g.create(NodeKind::Edit, Some(root)).unwrap();
        g.set(b, Prop::KeepsBlink, Value::Bool(true)).unwrap();
        g.focus_edit(Some(a), 1500).unwrap();
        g.input(InputEvent::Key(Key::Tab), 1500);
        assert_eq!(g.focus(), Some(b));
        assert!(!g.caret_visible(b, 1500), "a restart would show the caret now");
    }

    #[test]
    fn tab_goes_on_from_a_button_stop_to_the_edit_after_it() {
        let (mut g, a, ok, _) = page_with_edit();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let c = g.create(NodeKind::Edit, Some(root)).unwrap();
        g.set(ok, Prop::TabStop, Value::Bool(true)).unwrap();
        g.focus_edit(Some(a), 0).unwrap();
        g.input(InputEvent::Key(Key::Tab), 0);
        assert_eq!(g.focus(), None, "on the button");
        g.input(InputEvent::Key(Key::Tab), 0);
        assert_eq!(g.focus(), Some(c));
        g.input(InputEvent::Key(Key::BackTab), 0);
        g.input(InputEvent::Key(Key::BackTab), 0);
        assert_eq!(g.focus(), Some(a), "back past the button");
    }

    #[test]
    fn a_hidden_or_disabled_focused_edit_types_nothing_until_it_shows_again() {
        let (mut g, edit, _, _) = page_with_edit();
        g.focus_edit(Some(edit), 0).unwrap();
        for prop in [Prop::Enabled, Prop::Visible] {
            g.set(edit, prop, Value::Bool(false)).unwrap();
            assert!(!g.input(InputEvent::Text("x".into()), 0).consumed);
            g.input(InputEvent::Key(Key::Backspace), 0);
            assert_eq!(text_of(&g, edit).0, "");
            g.set(edit, prop, Value::Bool(true)).unwrap();
        }
        g.input(InputEvent::Text("y".into()), 0);
        assert_eq!(text_of(&g, edit).0, "y", "the focus stayed");
    }

    #[test]
    fn enter_with_an_open_list_picks_its_row_and_reaches_no_default_button() {
        let (mut g, _, ok, _) = page_with_edit();
        let root = g.dialog_root(g.top_dialog().unwrap()).unwrap();
        let combo = g.create(NodeKind::Combo, Some(root)).unwrap();
        g.set(combo, Prop::Items, Value::Items(vec!["a".into(), "b".into()])).unwrap();
        g.open_popup(combo, 0).unwrap();
        assert!(g.input(InputEvent::Key(Key::Enter), 0).consumed);
        let events = g.drain_events();
        assert_eq!(g.popup(), None);
        assert!(!events.contains(&UiEvent::Activated { node: ok }), "{events:?}");
    }

    #[test]
    fn a_release_activates_button_takes_a_release_after_any_press() {
        let (mut g, a, b) = page_with_two_buttons();
        g.set(b, Prop::ReleaseActivates, Value::Bool(true)).unwrap();
        down(&mut g, 5);
        up(&mut g, 15);
        assert_eq!(activations(&mut g), vec![b], "pressed on a, released on b");
        g.input(InputEvent::PointerUp { x: 15, y: 5, button: MouseButton::Left }, 0);
        assert_eq!(activations(&mut g), vec![b], "a release with no press");
        down(&mut g, 15);
        up(&mut g, 15);
        assert_eq!(activations(&mut g), vec![b], "once for a click on it");
        down(&mut g, 15);
        up(&mut g, 5);
        assert!(activations(&mut g).is_empty(), "a plain button still needs its press");
        let _ = a;
    }

    #[test]
    fn a_press_moves_the_focus_to_a_focus_on_press_control_only() {
        let (mut g, a, b) = page_with_two_buttons();
        g.set(a, Prop::FocusOnPress, Value::Bool(true)).unwrap();
        down(&mut g, 15);
        assert_eq!(g.focused_control(), None, "a plain button leaves the focus");
        down(&mut g, 5);
        assert_eq!(g.focused_control(), Some(a));
        g.focus_control(b).unwrap();
        assert_eq!((g.focused_control(), g.focus()), (Some(b), None));
    }

    #[test]
    fn a_blur_on_press_edit_loses_the_focus_to_a_press_elsewhere_only() {
        let (mut g, a, _) = page_with_two_buttons();
        let root = g.tree().parent(a).unwrap().unwrap();
        let edit = g.create(NodeKind::Edit, Some(root)).unwrap();
        g.set(edit, Prop::Rect, Value::Rect(Rect::new(30, 0, 10, 10))).unwrap();
        g.set(edit, Prop::BlurOnPress, Value::Bool(true)).unwrap();
        down(&mut g, 35);
        assert_eq!(g.focus(), Some(edit));
        down(&mut g, 35);
        assert_eq!(g.focus(), Some(edit), "a press on it keeps it");
        down(&mut g, 100);
        assert_eq!(g.focus(), None, "a press on nothing takes it");
    }

    #[test]
    fn undrained_hovers_coalesce_to_the_node_under_the_pointer() {
        let (mut g, a, b) = page_with_two_buttons();
        for x in [5, 15, 5, 15] {
            g.input(InputEvent::PointerMove { x, y: 5 }, 0);
        }
        assert_eq!(g.drain_events(), [UiEvent::Hovered { node: b, entered: true }]);
        let _ = a;
    }
}
