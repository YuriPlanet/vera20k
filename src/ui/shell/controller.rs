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

use vera20k_gui::{Gui, InputEvent, MouseButton, NodeId, NodeKind, Prop, Rect, Value};

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
}

/// LIFO dialog stack. The keyboard route is the stack's key-accepting
/// dialogs in registration order.
pub struct DialogController {
    gui: Gui,
    stack: Vec<Instance>,
    epoch: Instant,
}

impl Default for DialogController {
    fn default() -> Self {
        Self { gui: Gui::new(4096), stack: Vec::new(), epoch: Instant::now() }
    }
}

impl std::fmt::Debug for DialogController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.stack.iter().map(|i| i.id)).finish()
    }
}

impl DialogController {
    pub fn push(&mut self, id: DialogId, accepts_keys: bool) {
        let root = self.gui.create(NodeKind::Dialog, None).expect("the shell stack stays far below the node budget");
        let dialog = self
            .gui
            .push_dialog(root, vera20k_gui::DialogKind::Page, None, None)
            .expect("a fresh Dialog node");
        self.stack.push(Instance { id, dialog, accepts_keys, controls: Vec::new(), disabled: BTreeSet::new() });
    }

    pub fn pop(&mut self) -> Option<DialogId> {
        let inst = self.stack.pop()?;
        let _ = self.gui.pop_dialog(inst.dialog);
        Some(inst.id)
    }

    /// Clear the stack and make `id` the sole active dialog (a shell transition).
    pub fn reset_to(&mut self, id: DialogId, accepts_keys: bool) {
        while self.pop().is_some() {}
        self.push(id, accepts_keys);
    }

    /// Make `id` the active dialog, resetting only when it is not already on top,
    /// so an in-progress press survives across the gesture.
    pub fn ensure_active(&mut self, id: DialogId, accepts_keys: bool) {
        if self.top_id() != Some(id) {
            self.reset_to(id, accepts_keys);
        }
    }

    pub fn top_id(&self) -> Option<DialogId> {
        self.stack.last().map(|i| i.id)
    }

    fn control_of(inst: &Instance, node: Option<NodeId>) -> Option<u16> {
        let node = node?;
        inst.controls.iter().find(|(_, n)| *n == node).map(|(id, _)| *id)
    }

    pub fn pressed(&self) -> Option<u16> {
        let inst = self.stack.last()?;
        Self::control_of(inst, self.gui.pressed_in(inst.dialog))
    }

    pub fn hovered(&self) -> Option<u16> {
        let inst = self.stack.last()?;
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
        let inst = self.stack.last()?;
        let ms = self.gui.hover_started_in(inst.dialog)?;
        Some(self.epoch + Duration::from_millis(ms))
    }

    /// Key-accepting dialogs in registration order (bottom to top).
    pub fn kbd_route(&self) -> Vec<DialogId> {
        self.stack.iter().filter(|i| i.accepts_keys).map(|i| i.id).collect()
    }

    /// Runtime-disable a control on the active dialog.
    pub fn set_disabled(&mut self, control: u16, disabled: bool) {
        if let Some(inst) = self.stack.last_mut() {
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
        }
    }

    /// Returns the activated control: released over the pressed one, enabled.
    pub fn on_pointer_up(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) -> Option<u16> {
        if !self.sync(buttons) {
            return None;
        }
        self.gui.input(InputEvent::PointerUp { x, y, button: MouseButton::Left }, self.now());
        let inst = self.stack.last()?;
        self.gui.drain_events().into_iter().find_map(|e| match e {
            vera20k_gui::UiEvent::Activated { node, .. } => Self::control_of(inst, Some(node)),
            _ => None,
        })
    }

    pub fn on_pointer_move(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) {
        if self.sync(buttons) {
            self.gui.input(InputEvent::PointerMove { x, y }, self.now());
            self.gui.drain_events();
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
        let Some(inst) = self.stack.last_mut() else { return false };
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
