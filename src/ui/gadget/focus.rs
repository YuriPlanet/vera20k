//! FocusState — the four native dispatch focus globals (sticky capture,
//! keyboard focus, hover, current list — study G3/G7/G17/G18) as ONE value
//! owned by the app driver (study §6.1). Removal/destruction clears hover too
//! — deliberately closing the G7 stale-pointer hazard; no leave-notification
//! fires for a dead handle.

use super::{GadgetHandle, ListId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusState {
    /// Mouse-capture holder (G17): re-dispatched every tick, even masked-0.
    pub sticky: Option<GadgetHandle>,
    /// Keyboard-focus holder (G18): receives `FLAG_KEYBOARD` events,
    /// bypassing the bounds test.
    pub keyboard: Option<GadgetHandle>,
    /// Hover holder (G7): updated by the tick's pre-dispatch hit-test only.
    pub hovered: Option<GadgetHandle>,
    /// The list last ticked; a mismatch triggers the G5 fresh-list reset.
    pub current_list: Option<ListId>,
}

impl FocusState {
    pub fn new() -> Self {
        Self::default()
    }

    /// G24 + study §6.1 closure: a removed/destroyed gadget releases capture,
    /// keyboard focus AND hover when it holds them. Called by every
    /// `GadgetList` removal path.
    pub fn on_removed(&mut self, handle: GadgetHandle) {
        if self.sticky == Some(handle) {
            self.sticky = None;
        }
        if self.keyboard == Some(handle) {
            self.keyboard = None;
        }
        if self.hovered == Some(handle) {
            self.hovered = None;
        }
    }

    /// G25 — clear the attached list: forget the current list so the next tick
    /// takes the G5 reset path (the sanctioned page-swap mechanism).
    #[cfg(test)]
    pub fn clear_attached_list(&mut self) {
        self.current_list = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_removed_clears_only_matching_slots() {
        let a = GadgetHandle(1);
        let b = GadgetHandle(2);
        let mut f = FocusState {
            sticky: Some(a),
            keyboard: Some(b),
            hovered: Some(a),
            current_list: Some(ListId(7)),
        };
        f.on_removed(a);
        assert_eq!(f.sticky, None, "capture released (G24)");
        assert_eq!(f.keyboard, Some(b), "other holder untouched");
        assert_eq!(f.hovered, None, "hover cleared too (study §6.1 closure)");
        assert_eq!(f.current_list, Some(ListId(7)), "list identity untouched");
    }

    #[test]
    fn clear_attached_list_only_clears_list() {
        let a = GadgetHandle(1);
        let mut f = FocusState {
            sticky: Some(a),
            keyboard: None,
            hovered: Some(a),
            current_list: Some(ListId(1)),
        };
        f.clear_attached_list();
        assert_eq!(f.current_list, None, "G25 zeroes only current_list");
        assert_eq!(f.sticky, Some(a));
        assert_eq!(f.hovered, Some(a));
    }
}
