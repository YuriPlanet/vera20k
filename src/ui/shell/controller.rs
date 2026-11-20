//! Shell dialog stack and button input, run on the `vera20k-gui` tree.
//!
//! Each pushed dialog is a `Dialog` node; each control a caller feeds in is a
//! `Button` node keyed by its resource id, created on first sight. Every
//! pointer call syncs the fed rects into the tree (a control left out of the
//! feed is hidden), so hit-testing, capture and hover are the GUI crate's
//! (612B70: release activates only on the captured, enabled control; a
//! disabled control still hovers, so it keeps driving its tooltip).
//!
//! Callers feed only owner-draw BUTTON rects: statics are never hit-tested,
//! which keeps `pressed` button-only (the main-menu mouse-down sound).

use std::collections::BTreeSet;

use vera20k_gui::{Gui, InputEvent, MouseButton, NodeId, NodeKind, Prop, Rect, UiEvent, Value};

use super::descriptor::DialogId;
use super::layout::LaidOutControl;
use crate::util::time::{Duration, Instant};

/// Keyboard key the controller routes (contract C3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKey {
    Tab,
    Enter,
    Escape,
}

struct Instance {
    id: DialogId,
    dialog: vera20k_gui::DialogId,
    accepts_keys: bool,
    /// Resource id → button node, in first-fed order.
    controls: Vec<(u16, NodeId)>,
    disabled: BTreeSet<u16>,
    /// A box over whatever screen is up (the WOL page dialogs): it stays on
    /// top, with its widgets, through the screen's pushes, pops and resets,
    /// until its owner [`DialogController::close`]s it.
    overlay: bool,
}

/// LIFO dialog stack. The keyboard route is the stack's key-accepting
/// dialogs in registration order.
pub struct DialogController {
    gui: Gui,
    stack: Vec<Instance>,
    epoch: Instant,
    /// Events of the nodes a screen built itself (not the fed buttons),
    /// waiting for that screen to drain them.
    pending: Vec<UiEvent>,
}

impl Default for DialogController {
    fn default() -> Self {
        let mut gui = Gui::new(4096);
        let (time, width, height) = super::saved_file_input::host_double_click_limits();
        gui.set_double_click(time.as_millis() as u64, width, height);
        Self { gui, stack: Vec::new(), epoch: Instant::now(), pending: Vec::new() }
    }
}

impl std::fmt::Debug for DialogController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.stack.iter().map(|i| i.id)).finish()
    }
}

impl DialogController {
    pub fn push(&mut self, id: DialogId, accepts_keys: bool) {
        self.push_kind(id, accepts_keys, vera20k_gui::DialogKind::Page, false);
    }

    /// [`Self::push`] as a modal: it blocks every dialog beneath it.
    pub fn push_modal(&mut self, id: DialogId, accepts_keys: bool) {
        self.push_kind(id, accepts_keys, vera20k_gui::DialogKind::Modal, false);
    }

    /// [`Self::push`] as an overlay: it stays on top until [`Self::close`].
    pub fn push_overlay(&mut self, id: DialogId) {
        self.push_kind(id, false, vera20k_gui::DialogKind::Page, true);
    }

    /// A dialog goes under the overlays, an overlay on top.
    fn push_kind(&mut self, id: DialogId, accepts_keys: bool, kind: vera20k_gui::DialogKind, overlay: bool) {
        let root = self.gui.create(NodeKind::Dialog, None).expect("the shell stack stays far below the node budget");
        let dialog = self.gui.push_dialog(root, kind, None, None).expect("a fresh Dialog node");
        let at = match overlay {
            true => self.stack.len(),
            false => self.stack.iter().position(|i| i.overlay).unwrap_or(self.stack.len()),
        };
        self.stack.insert(at, Instance { id, dialog, accepts_keys, controls: Vec::new(), disabled: BTreeSet::new(), overlay });
        for inst in &self.stack[at + 1..] {
            let _ = self.gui.raise_dialog(inst.dialog);
        }
    }

    /// Pops the topmost dialog under the overlays.
    pub fn pop(&mut self) -> Option<DialogId> {
        let at = self.stack.iter().rposition(|i| !i.overlay)?;
        Some(self.remove(at))
    }

    /// Closes the topmost instance of `id`, an overlay included.
    pub fn close(&mut self, id: DialogId) {
        if let Some(at) = self.stack.iter().rposition(|i| i.id == id) {
            self.remove(at);
        }
    }

    fn remove(&mut self, at: usize) -> DialogId {
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

    /// Control `id` under the topmost live instance of `dialog`.
    pub fn control(&self, dialog: DialogId, id: u16) -> Option<NodeId> {
        self.gui.find_id(self.root_in(dialog)?, i32::from(id))
    }

    /// Control `id`'s `prop` on `dialog`.
    pub fn prop(&self, dialog: DialogId, id: u16, prop: Prop) -> Option<&Value> {
        self.gui.get(self.control(dialog, id)?, prop).ok().flatten()
    }

    /// Control `id`'s boolean `prop` on `dialog` (`default` when unset).
    pub fn flag(&self, dialog: DialogId, id: u16, prop: Prop, default: bool) -> bool {
        match self.prop(dialog, id, prop) {
            Some(Value::Bool(on)) => *on,
            _ => default,
        }
    }

    /// Sets `prop` on control `id` of `dialog`.
    pub fn set_prop(&mut self, dialog: DialogId, id: u16, prop: Prop, value: Value) {
        if let Some(node) = self.control(dialog, id) {
            let _ = self.gui.set(node, prop, value);
        }
    }

    /// Control `id`'s text on `dialog` ("" when it has none).
    pub fn text(&self, dialog: DialogId, id: u16) -> String {
        match self.prop(dialog, id, Prop::Text) {
            Some(Value::Text(text)) => text.clone(),
            _ => String::new(),
        }
    }

    /// Control `id`'s text as a program assigns it (`SetWindowTextW`): the
    /// caret goes to the end and the view to the start.
    pub fn set_text(&mut self, dialog: DialogId, id: u16, text: &str) {
        self.set_prop(dialog, id, Prop::Text, Value::Text(text.to_owned()));
        self.set_prop(dialog, id, Prop::Caret, Value::Int(text.chars().count() as i32));
        self.set_prop(dialog, id, Prop::Top, Value::Int(0));
    }

    /// `SetFocus` on edit `id` of `dialog` (none clears it), on top or not.
    pub fn focus_control(&mut self, dialog: DialogId, id: Option<u16>) {
        let now = self.now();
        let node = id.and_then(|id| self.control(dialog, id));
        if let Some(inst) = self.instance(dialog).map(|inst| inst.dialog) {
            let _ = self.gui.focus_edit_in(inst, node, now);
        }
    }

    /// The control id of the edit holding `dialog`'s focus.
    pub fn focused_control(&self, dialog: DialogId) -> Option<u16> {
        let node = self.gui.focus_in(self.instance(dialog)?.dialog)?;
        self.gui.id_of(node).and_then(|id| u16::try_from(id).ok())
    }

    /// Drops `dialog`'s capture and hover only; a box over it keeps its own.
    pub fn drop_pointer_in(&mut self, dialog: DialogId) {
        if let Some(inst) = self.instance(dialog).map(|inst| inst.dialog) {
            self.gui.drop_pointer_in(inst);
        }
    }

    /// The root node of the topmost live instance of `id`.
    pub fn root_in(&self, id: DialogId) -> Option<NodeId> {
        self.gui.dialog_root(self.instance(id)?.dialog)
    }

    /// Sets the device-to-authored mapping the top dialog's widgets use.
    pub fn set_scale(&mut self, scale: crate::ui::shell::geom::ShellScale) {
        if let Some(inst) = self.base() {
            self.gui.set_dialog_scale(inst.dialog, scale);
        }
    }

    /// [`Self::set_scale`] of dialog `id`, an overlay included.
    pub fn set_scale_in(&mut self, id: DialogId, scale: crate::ui::shell::geom::ShellScale) {
        if let Some(inst) = self.instance(id) {
            self.gui.set_dialog_scale(inst.dialog, scale);
        }
    }

    /// Events of the screen's own widgets since the last drain, in order.
    pub fn drain_events(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.pending)
    }

    /// Wheel, keys and text for the top dialog's widgets; true when consumed.
    pub fn on_input(&mut self, event: InputEvent) -> bool {
        if self.stack.is_empty() {
            return false;
        }
        let consumed = self.gui.input(event, self.now()).consumed;
        self.collect();
        consumed
    }

    /// Widget timers (list scroll repeat); the next deadline.
    pub fn poll(&mut self) -> Option<Instant> {
        let next = self.gui.poll(self.now());
        self.collect();
        next.map(|ms| self.epoch + Duration::from_millis(ms))
    }

    /// Moves the GUI's new events to `pending`, except those of fed buttons
    /// (their callers read press and release through this controller).
    /// Returns the fed control an activation named.
    fn collect(&mut self) -> Option<u16> {
        let mut activated = None;
        let inst = self.stack.iter().rev().find(|i| !i.overlay);
        for event in self.gui.drain_events() {
            let node = match &event {
                UiEvent::Activated { node, .. }
                | UiEvent::Hovered { node, .. }
                | UiEvent::Pressed { node } => Some(*node),
                _ => None,
            };
            let fed = inst.and_then(|i| Self::control_of(i, node));
            match (fed, &event) {
                (Some(id), UiEvent::Activated { .. }) => activated = Some(id),
                (Some(_), _) => {}
                (None, _) => self.pending.push(event),
            }
        }
        activated
    }

    /// Clear the stack and make `id` the sole active dialog (a shell transition).
    pub fn reset_to(&mut self, id: DialogId, accepts_keys: bool) {
        while self.pop().is_some() {}
        self.pending.clear();
        self.push(id, accepts_keys);
    }

    /// Make `id` the active dialog, resetting only when it is not already on top,
    /// so an in-progress press survives across the gesture.
    pub fn ensure_active(&mut self, id: DialogId, accepts_keys: bool) {
        if self.top_id() != Some(id) {
            self.reset_to(id, accepts_keys);
        }
    }

    /// The screen's top dialog: overlays over it do not count, as a box
    /// kept beside the stack would not.
    pub fn top_id(&self) -> Option<DialogId> {
        self.base().map(|i| i.id)
    }

    /// The topmost overlay, which takes the input over the screen.
    pub fn overlay_id(&self) -> Option<DialogId> {
        self.stack.iter().rev().find(|i| i.overlay).map(|i| i.id)
    }

    fn base(&self) -> Option<&Instance> {
        self.stack.iter().rev().find(|i| !i.overlay)
    }

    fn control_of(inst: &Instance, node: Option<NodeId>) -> Option<u16> {
        let node = node?;
        inst.controls.iter().find(|(_, n)| *n == node).map(|(id, _)| *id)
    }

    pub fn pressed(&self) -> Option<u16> {
        let inst = self.base()?;
        Self::control_of(inst, self.gui.pressed_in(inst.dialog))
    }

    pub fn hovered(&self) -> Option<u16> {
        let inst = self.base()?;
        Self::control_of(inst, self.gui.hovered_in(inst.dialog))
    }

    /// Press state of the topmost live instance of `id`, so a dialog keeps
    /// painting its own pressed button while a box sits over it.
    pub fn pressed_in(&self, id: DialogId) -> Option<u16> {
        let inst = self.instance(id)?;
        Self::control_of(inst, self.gui.pressed_in(inst.dialog))
    }

    pub fn hovered_in(&self, id: DialogId) -> Option<u16> {
        let inst = self.instance(id)?;
        Self::control_of(inst, self.gui.hovered_in(inst.dialog))
    }

    /// The node captured in the topmost live instance of `id`, a screen's
    /// own widget included (fed buttons answer [`Self::pressed_in`]).
    pub fn pressed_node_in(&self, id: DialogId) -> Option<NodeId> {
        self.gui.pressed_in(self.instance(id)?.dialog)
    }

    /// The node under the pointer in the topmost live instance of `id`.
    pub fn hovered_node_in(&self, id: DialogId) -> Option<NodeId> {
        self.gui.hovered_in(self.instance(id)?.dialog)
    }

    fn instance(&self, id: DialogId) -> Option<&Instance> {
        self.stack.iter().rev().find(|i| i.id == id)
    }

    /// Give every live instance of `id` fresh press/hover/enable state (a new
    /// dialog instance in the same stack slot).
    pub fn restart_instance(&mut self, id: DialogId) {
        for inst in self.stack.iter_mut().filter(|i| i.id == id) {
            self.gui.reset_pointer(inst.dialog);
            inst.disabled.clear();
        }
    }

    pub fn hover_started_at(&self) -> Option<Instant> {
        let inst = self.base()?;
        let ms = self.gui.hover_started_in(inst.dialog)?;
        Some(self.epoch + Duration::from_millis(ms))
    }

    /// Key-accepting dialogs in registration order (bottom to top).
    pub fn kbd_route(&self) -> Vec<DialogId> {
        self.stack.iter().filter(|i| i.accepts_keys).map(|i| i.id).collect()
    }

    /// Runtime-disable a control on the active dialog.
    pub fn set_disabled(&mut self, control: u16, disabled: bool) {
        if let Some(inst) = self.stack.iter_mut().rev().find(|i| !i.overlay) {
            if disabled {
                inst.disabled.insert(control);
            } else {
                inst.disabled.remove(&control);
            }
        }
    }

    pub fn on_pointer_down(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) {
        if self.sync(buttons) {
            self.gui.input(InputEvent::PointerDown { x, y, button: MouseButton::Left }, self.now());
            self.collect();
        }
    }

    /// Returns the activated control: released over the pressed one, enabled.
    pub fn on_pointer_up(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) -> Option<u16> {
        if !self.sync(buttons) {
            return None;
        }
        self.gui.input(InputEvent::PointerUp { x, y, button: MouseButton::Left }, self.now());
        self.collect()
    }

    pub fn on_pointer_move(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) {
        if self.sync(buttons) {
            self.gui.input(InputEvent::PointerMove { x, y }, self.now());
            self.collect();
        }
    }

    /// Route a key; true when a key-accepting dialog consumed Enter/Escape.
    pub fn on_key(&mut self, key: ShellKey) -> bool {
        matches!(key, ShellKey::Enter | ShellKey::Escape) && self.stack.iter().any(|i| i.accepts_keys)
    }

    fn now(&self) -> u64 {
        Instant::now().saturating_duration_since(self.epoch).as_millis() as u64
    }

    /// Mirror the fed rects and the runtime-disabled set into the top dialog's
    /// button nodes. False when no dialog is active.
    fn sync(&mut self, buttons: &[LaidOutControl]) -> bool {
        let Some(inst) = self.stack.iter_mut().rev().find(|i| !i.overlay) else { return false };
        let Some(root) = self.gui.dialog_root(inst.dialog) else { return false };
        for c in buttons {
            if !inst.controls.iter().any(|(id, _)| *id == c.id) {
                let node = self.gui.create(NodeKind::Button, Some(root)).expect("budget");
                inst.controls.push((c.id, node));
            }
        }
        for (id, node) in &inst.controls {
            let fed = buttons.iter().find(|c| c.id == *id);
            let rect = fed.map(|c| Rect::new(c.rect.x, c.rect.y, c.rect.w, c.rect.h)).unwrap_or_default();
            let _ = self.gui.set(*node, Prop::Rect, Value::Rect(rect));
            let _ = self.gui.set(*node, Prop::Visible, Value::Bool(fed.is_some()));
            let _ = self.gui.set(*node, Prop::Enabled, Value::Bool(!inst.disabled.contains(id)));
        }
        true
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::shell::geom::RectPx;

    const A: DialogId = DialogId(0x00E2);
    const B: DialogId = DialogId(0x0100);

    fn two_buttons() -> Vec<LaidOutControl> {
        vec![
            LaidOutControl {
                id: 1,
                rect: RectPx::new(0, 0, 10, 10),
            },
            LaidOutControl {
                id: 2,
                rect: RectPx::new(10, 0, 10, 10),
            },
        ]
    }

    #[test]
    fn press_must_match_release() {
        let b = two_buttons();
        let mut c = DialogController::default();
        c.ensure_active(A, false);
        // Press button 1, release on button 2 -> no fire.
        c.on_pointer_down(5, 5, &b);
        assert_eq!(c.pressed(), Some(1));
        assert_eq!(c.on_pointer_up(15, 5, &b), None);
        // Press and release same button -> fire.
        c.on_pointer_down(5, 5, &b);
        assert_eq!(c.on_pointer_up(5, 5, &b), Some(1));
        // Release outside all buttons -> no fire.
        c.on_pointer_down(5, 5, &b);
        assert_eq!(c.on_pointer_up(50, 50, &b), None);
    }

    #[test]
    fn disabled_control_suppresses_press_but_still_hovers() {
        let b = two_buttons();
        let mut c = DialogController::default();
        c.ensure_active(B, false);
        c.set_disabled(2, true);
        // Press over disabled button 2 -> no pressed state, no fire.
        c.on_pointer_down(15, 5, &b);
        assert_eq!(c.pressed(), None);
        assert_eq!(c.on_pointer_up(15, 5, &b), None);
        // ...but hover still tracks the disabled button and arms its timer.
        c.on_pointer_move(15, 5, &b);
        assert_eq!(c.hovered(), Some(2));
        assert!(c.hover_started_at().is_some());
        // Re-enable -> fires.
        c.set_disabled(2, false);
        c.on_pointer_down(15, 5, &b);
        assert_eq!(c.on_pointer_up(15, 5, &b), Some(2));
    }

    #[test]
    fn kbd_route_is_registration_order_and_pruned_on_pop() {
        let mut c = DialogController::default();
        c.push(A, true);
        c.push(B, true);
        assert_eq!(c.kbd_route(), &[A, B]); // registration order, not LIFO
        c.pop();
        assert_eq!(c.kbd_route(), &[A]);
        assert!(c.on_key(ShellKey::Escape));
        c.pop();
        assert!(!c.on_key(ShellKey::Escape));
    }

    #[test]
    fn key_route_consumes_modal_enter_and_escape_only() {
        let mut c = DialogController::default();
        assert!(!c.on_key(ShellKey::Enter));
        c.ensure_active(A, false);
        assert!(!c.on_key(ShellKey::Enter));
        c.ensure_active(B, true);
        assert!(c.on_key(ShellKey::Enter));
        assert!(c.on_key(ShellKey::Escape));
        assert!(!c.on_key(ShellKey::Tab));
    }

    #[test]
    fn an_overlay_stays_on_top_with_its_widgets_through_resets() {
        let page = DialogId(0x00CF);
        let mut c = DialogController::default();
        c.ensure_active(A, false);
        c.push_overlay(page);
        let edit = c.gui.create(NodeKind::Edit, c.root_in(page)).unwrap();
        c.ensure_active(B, false);
        c.push(A, false);
        // The screen's own queries see its box, as when the page lived
        // beside the stack; the GUI routes the input to the page.
        assert_eq!((c.top_id(), c.overlay_id()), (Some(A), Some(page)));
        assert_eq!(c.gui.top_dialog(), c.instance(page).map(|i| i.dialog));
        assert_eq!(c.pop(), Some(A));
        assert!(c.gui.tree().kind(edit).is_ok(), "the overlay kept its widgets");
        c.close(page);
        assert_eq!((c.top_id(), c.root_in(page)), (Some(B), None));
    }

    #[test]
    fn push_over_base_pops_back_to_it() {
        let modal = DialogId(0x0120);
        let mut c = DialogController::default();
        c.ensure_active(A, false);
        c.push(modal, true);
        assert_eq!(c.top_id(), Some(modal));
        assert_eq!(c.kbd_route(), &[modal], "accepts_keys registers the route");
        assert_eq!(c.pop(), Some(modal));
        assert_eq!(c.top_id(), Some(A), "LIFO pop restores the shell beneath");
        assert!(c.kbd_route().is_empty(), "route pruned on pop");
    }

    #[test]
    fn ensure_active_resets_only_on_dialog_change() {
        let b = two_buttons();
        let mut c = DialogController::default();
        c.ensure_active(A, false);
        c.on_pointer_down(5, 5, &b);
        assert_eq!(c.pressed(), Some(1));
        c.ensure_active(A, false); // same dialog -> no reset
        assert_eq!(c.pressed(), Some(1)); // press survives the gesture
        c.ensure_active(B, false); // changed -> reset
        assert_eq!(c.pressed(), None);
    }
}
