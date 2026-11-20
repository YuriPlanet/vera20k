//! `Gui`: the tree plus dialog stack, pointer capture and the event queue
//! (docs/gui-library-api.md §2.1, §3, §6, §8).
//!
//! Button semantics are the shell's owner-draw button (612B70): a press
//! captures the button, the pushed look shows only while the pointer is still
//! over it, and release activates only on the captured button. A disabled
//! button still takes hover (it shows tooltip and status text) but no press.

use crate::geom::Rect;
use crate::node::{NodeId, NodeKind, Prop, Refused, Tree, Value};

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
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    PointerDown { x: i32, y: i32, button: MouseButton },
    PointerUp { x: i32, y: i32, button: MouseButton },
    PointerMove { x: i32, y: i32 },
    Key(Key),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InputResult {
    pub consumed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    Activated { node: NodeId, command: Option<String> },
    Hovered { node: NodeId, entered: bool },
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
    hover_started: Option<UiMs>,
}

struct Dialog {
    id: DialogId,
    root: NodeId,
    kind: DialogKind,
    default_button: Option<NodeId>,
    cancel_button: Option<NodeId>,
    pointer: Pointer,
}

pub struct Gui {
    tree: Tree,
    dialogs: Vec<Dialog>,
    next_dialog: u32,
    events: Vec<UiEvent>,
}

impl Gui {
    pub fn new(max_nodes: usize) -> Self {
        Gui {
            tree: Tree::new(max_nodes),
            dialogs: Vec::new(),
            next_dialog: 0,
            events: Vec::new(),
        }
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn create(&mut self, kind: NodeKind, parent: Option<NodeId>) -> Result<NodeId, Refused> {
        self.tree.create(kind, parent)
    }

    pub fn set(&mut self, id: NodeId, prop: Prop, value: Value) -> Result<(), Refused> {
        self.tree.set(id, prop, value)
    }

    pub fn get(&self, id: NodeId, prop: Prop) -> Result<Option<&Value>, Refused> {
        self.tree.get(id, prop)
    }

    pub fn remove(&mut self, id: NodeId) -> Result<(), Refused> {
        self.tree.remove(id)?;
        let tree = &self.tree;
        self.dialogs.retain(|d| tree.kind(d.root).is_ok());
        for d in &mut self.dialogs {
            for slot in [&mut d.pointer.pressed, &mut d.pointer.hovered] {
                if slot.is_some_and(|n| tree.kind(n).is_err()) {
                    *slot = None;
                }
            }
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
        self.dialogs.push(Dialog { id, root, kind, default_button, cancel_button, pointer: Pointer::default() });
        Ok(id)
    }

    /// Pops the dialog and removes its nodes.
    pub fn pop_dialog(&mut self, id: DialogId) -> Result<(), Refused> {
        let pos = self.dialogs.iter().position(|d| d.id == id).ok_or(Refused::NotFound)?;
        let d = self.dialogs.remove(pos);
        self.remove(d.root)
    }

    pub fn top_dialog(&self) -> Option<DialogId> {
        self.dialogs.last().map(|d| d.id)
    }

    pub fn dialog_root(&self, id: DialogId) -> Option<NodeId> {
        self.dialogs.iter().find(|d| d.id == id).map(|d| d.root)
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

    pub fn pressed_in(&self, id: DialogId) -> Option<NodeId> {
        self.dialog(id).and_then(|d| d.pointer.pressed)
    }

    pub fn hovered_in(&self, id: DialogId) -> Option<NodeId> {
        self.dialog(id).and_then(|d| d.pointer.hovered)
    }

    pub fn hover_started_in(&self, id: DialogId) -> Option<UiMs> {
        self.dialog(id).and_then(|d| d.pointer.hover_started)
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

    pub fn hover_started(&self) -> Option<UiMs> {
        self.top_pointer().hover_started
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
        match event {
            InputEvent::PointerMove { x, y } => {
                let over = self.hit(root, x, y);
                self.set_hover(over, now);
                InputResult { consumed: over.is_some() || modal }
            }
            InputEvent::PointerDown { x, y, button: MouseButton::Left } => {
                let over = self.hit(root, x, y);
                let pressed = over.filter(|n| self.tree.effective(*n, Prop::Enabled));
                self.top_mut().pointer.pressed = pressed;
                InputResult { consumed: over.is_some() || modal }
            }
            InputEvent::PointerUp { x, y, button: MouseButton::Left } => {
                let over = self.hit(root, x, y);
                let held = self.top_mut().pointer.pressed.take();
                if let Some(n) = held.filter(|h| Some(*h) == over) {
                    if self.tree.effective(n, Prop::Enabled) {
                        self.activate(n);
                    }
                }
                InputResult { consumed: held.is_some() || over.is_some() || modal }
            }
            InputEvent::PointerDown { .. } | InputEvent::PointerUp { .. } => {
                InputResult { consumed: modal }
            }
            InputEvent::Key(key) => self.key(key),
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
            Key::Tab => None,
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
        let command = self.tree.node(node).ok().and_then(|n| n.text_prop(Prop::Command)).map(str::to_owned);
        self.events.push(UiEvent::Activated { node, command });
    }

    fn set_hover(&mut self, over: Option<NodeId>, now: UiMs) {
        let pointer = &mut self.top_mut().pointer;
        if pointer.hovered == over {
            return;
        }
        let old = pointer.hovered;
        pointer.hovered = over;
        pointer.hover_started = over.map(|_| now);
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
        let mut found = None;
        self.walk(root, &mut |id, kind| {
            if found.is_none()
                && kind.is_focusable()
                && self.tree.effective(id, Prop::Visible)
                && self.tree.screen_rect(id).is_ok_and(|r: Rect| r.contains(x, y))
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

/// 612B70's timer highlight: 1000 ms alternating, starting off.
pub fn hover_flash_on(started: UiMs, now: UiMs) -> bool {
    now.saturating_sub(started) / 1000 % 2 == 1
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
        g.set(a, Prop::Command, Value::Text("A".into())).unwrap();
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
        assert_eq!(g.hover_started(), Some(7));
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
    fn hover_flash_starts_off_and_alternates_each_second() {
        assert!(!hover_flash_on(100, 1099));
        assert!(hover_flash_on(100, 1100));
        assert!(!hover_flash_on(100, 2100));
    }
}
