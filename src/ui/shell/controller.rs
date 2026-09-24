//! Descriptor-driven shell dialog input controller (substrate Slice 2).
//!
//! Owns the press/hover/enable state for the active front-end shell dialog and
//! the press-must-match-release gesture, plus a dialog stack and a keyboard route
//! in registration order for the wider lifecycle contract. Render-agnostic:
//! operates on the button rects the layout pass produced (`LaidOutControl`) and
//! plain resource ids, so it honors the ui/ layering rule (no render/assets/sim).
//!
//! The two migrated shells (main menu 0xE2, single player 0x100) feed only their
//! owner-draw BUTTON rects in. Statics (title/website) are never fed, so they are
//! never hit-tested or hover-tracked — matching the current per-shell hit-tests
//! that scan only the button array, and keeping `pressed` button-only so the
//! main-menu mouse-down sound never trips on the website static. Two hit-tests are
//! kept: the press path skips runtime-disabled controls (single-player Load Saved
//! Game when no saves exist); the hover path does NOT, so a disabled button still
//! drives its tooltip/timer exactly as before.

use std::collections::BTreeSet;
use std::time::Instant;

use super::descriptor::DialogId;
use super::layout::LaidOutControl;

/// Keyboard key the controller routes (contract C3). The two migrated shells
/// register no keyboard controls, so these are inert for Slice 2; the plumbing
/// exists so stacked modals (Slice 5) and skirmish (Slice 4) inherit it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKey {
    Tab,
    Enter,
    Escape,
}

/// One live dialog: its id plus the per-control runtime state the descriptor does
/// not carry (descriptor `enabled` is the static template default only).
#[derive(Debug, Clone)]
struct DialogInstance {
    id: DialogId,
    /// Control pressed but not yet released — the press-must-match-release identity.
    pressed: Option<u16>,
    /// Control under the cursor (drives the bottom-left tooltip/status line).
    hovered: Option<u16>,
    /// Wall-clock of the last hover transition (single-player hover-delay parity).
    hover_started_at: Option<Instant>,
    /// Runtime-disabled control ids (single-player Load Saved Game when no saves).
    disabled: BTreeSet<u16>,
    /// Whether this dialog registered into the keyboard route on push.
    accepts_keys: bool,
}

impl DialogInstance {
    fn new(id: DialogId, accepts_keys: bool) -> Self {
        Self {
            id,
            pressed: None,
            hovered: None,
            hover_started_at: None,
            disabled: BTreeSet::new(),
            accepts_keys,
        }
    }
}

/// The shared shell input router.
#[derive(Debug, Default)]
pub struct DialogController {
    /// LIFO dialog stack: top = focused dialog. Push on open, pop on teardown.
    stack: Vec<DialogInstance>,
    /// Keyboard route in REGISTRATION order (C3) — NOT stack order. Maintained as
    /// a separate list appended on push and pruned on pop.
    kbd_route: Vec<DialogId>,
}

impl DialogController {
    /// Push a new dialog (C1): append to the LIFO stack and register the keyboard
    /// route if it accepts keys. Pressed/hover start clear.
    pub fn push(&mut self, id: DialogId, accepts_keys: bool) {
        self.stack.push(DialogInstance::new(id, accepts_keys));
        if accepts_keys {
            self.kbd_route.push(id);
        }
    }

    /// Pop the top dialog (C5): LIFO compact + prune its keyboard-route entry.
    /// Focus implicitly returns to the new top (or the game window if empty).
    pub fn pop(&mut self) -> Option<DialogId> {
        let inst = self.stack.pop()?;
        if inst.accepts_keys {
            if let Some(pos) = self.kbd_route.iter().rposition(|d| *d == inst.id) {
                self.kbd_route.remove(pos);
            }
        }
        Some(inst.id)
    }

    /// Replace the whole stack with a single base dialog.
    pub fn reset_to(&mut self, id: DialogId, accepts_keys: bool) {
        self.stack.clear();
        self.kbd_route.clear();
        self.push(id, accepts_keys);
    }

    /// Make `id` the active (top) dialog, resetting input state ONLY if the active
    /// dialog actually changed. A no-op when `id` is already on top, so press/hover
    /// state survives across the down/up of a single gesture (and the single-player
    /// disabled override is not cleared mid-gesture).
    pub fn ensure_active(&mut self, id: DialogId, accepts_keys: bool) {
        if self.top_id() != Some(id) {
            self.reset_to(id, accepts_keys);
        }
    }

    pub fn top_id(&self) -> Option<DialogId> {
        self.stack.last().map(|i| i.id)
    }

    /// The pressed control on the top dialog (button-only by construction, since
    /// only button rects are fed in).
    pub fn pressed(&self) -> Option<u16> {
        self.stack.last().and_then(|i| i.pressed)
    }

    pub fn hovered(&self) -> Option<u16> {
        self.stack.last().and_then(|i| i.hovered)
    }

    pub fn hover_started_at(&self) -> Option<Instant> {
        self.stack.last().and_then(|i| i.hover_started_at)
    }

    /// The keyboard route in registration order (contract C3).
    pub fn kbd_route(&self) -> &[DialogId] {
        &self.kbd_route
    }

    /// Runtime enable/disable of one control on the top dialog. Layers over the
    /// descriptor's static default (single-player Load Saved Game guard).
    pub fn set_disabled(&mut self, control: u16, disabled: bool) {
        if let Some(inst) = self.stack.last_mut() {
            if disabled {
                inst.disabled.insert(control);
            } else {
                inst.disabled.remove(&control);
            }
        }
    }

    /// Pointer-down on the top dialog: record the pressed control (enable-filtered).
    pub fn on_pointer_down(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) {
        if let Some(inst) = self.stack.last_mut() {
            inst.pressed = Self::hit_press(inst, x, y, buttons);
        }
    }

    /// Pointer-up on the top dialog. Fires (returns the control id) ONLY when the
    /// release lands on the same control that was pressed and that control is not
    /// runtime-disabled. Down/Move never fire.
    pub fn on_pointer_up(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) -> Option<u16> {
        let inst = self.stack.last_mut()?;
        // Release is hit-tested UNfiltered (matching the per-shell mouse_up); the
        // disabled control is rejected by the explicit guard below, exactly as the
        // single-player release re-guard does.
        let released = Self::hit_any(x, y, buttons);
        let pressed = inst.pressed.take();
        if pressed.is_some() && pressed == released {
            let control = released.expect("pressed/released checked above");
            if inst.disabled.contains(&control) {
                None
            } else {
                Some(control)
            }
        } else {
            None
        }
    }

    /// Pointer-move on the top dialog: update hover (UNfiltered — a disabled button
    /// still hover-tracks and arms its timer, matching the single-player move path).
    pub fn on_pointer_move(&mut self, x: i32, y: i32, buttons: &[LaidOutControl]) {
        if let Some(inst) = self.stack.last_mut() {
            let new_hover = Self::hit_any(x, y, buttons);
            if inst.hovered != new_hover {
                inst.hovered = new_hover;
                inst.hover_started_at = new_hover.map(|_| Instant::now());
            }
        }
    }

    /// Keyboard event routed in `kbd_route` registration order (C3), independent of
    /// the LIFO focus stack. Message-box modals consume Enter/Escape through this
    /// route; the host app resolves the resulting dialog action.
    pub fn on_key(&mut self, key: ShellKey) -> bool {
        matches!(key, ShellKey::Enter | ShellKey::Escape) && !self.kbd_route.is_empty()
    }

    /// Press-path hit-test: the FIRST button containing the point, then suppressed
    /// to `None` if that button is runtime-disabled. Byte-identical to the per-shell
    /// mouse-down — it finds the first containing control and drops it when disabled;
    /// it does NOT skip a disabled control to a later one beneath the same point
    /// (which would only differ if button rects overlapped, but it stays faithful).
    fn hit_press(inst: &DialogInstance, x: i32, y: i32, buttons: &[LaidOutControl]) -> Option<u16> {
        match Self::hit_any(x, y, buttons) {
            Some(id) if inst.disabled.contains(&id) => None,
            other => other,
        }
    }

    /// Hover/release hit-test: first button containing the point, INCLUDING
    /// disabled (a disabled button still hover-tracks for its tooltip).
    fn hit_any(x: i32, y: i32, buttons: &[LaidOutControl]) -> Option<u16> {
        buttons.iter().find(|c| c.rect.contains(x, y)).map(|c| c.id)
    }
}

#[cfg(test)]
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
    fn stack_push_pop_focus_restore() {
        let mut c = DialogController::default();
        c.push(A, false);
        c.push(B, false);
        assert_eq!(c.top_id(), Some(B));
        assert_eq!(c.pop(), Some(B));
        assert_eq!(c.top_id(), Some(A)); // focus restored to parent
        assert_eq!(c.pop(), Some(A));
        assert_eq!(c.top_id(), None); // back to the game window
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
