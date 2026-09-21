//! Retained gadget list: insertion order = hit-test priority = draw order
//! (G1/G14/G20/O7). Vec-backed with stable per-list handles; gadgets belong to
//! exactly one list by ownership (the G2 "every insert self-removes first"
//! invariant is structural in Rust — specs are values, handles are per-list).

use super::focus::FocusState;
use super::{FLAG_KEYBOARD, GadgetHandle, GadgetRect, ListId, STICKY_CTOR_MASK};

/// Toggle kind (G22 row 5): 0 = no on-state, 1 = flip, 2 = latch-ON only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToggleKind {
    #[default]
    Plain,
    Flip,
    LatchOn,
}

/// Per-button toggle state — the G22 pressed/on/kind triple (verified native
/// field identities cited in the plan Sources section).
#[derive(Debug, Clone, Copy, Default)]
pub struct ButtonState {
    pub is_pressed: bool,
    pub is_on: bool,
    pub kind: ToggleKind,
}

/// Which Action implementation a gadget runs (the live behavior shapes; cameo
/// + click-region added in A2/A3).
#[derive(Debug, Clone, Copy)]
pub enum GadgetBehavior {
    /// Base action (G16): consume any masked flags.
    Plain,
    /// Control action (G13): post `id|0x8000` then base.
    Control,
    /// Toggle-button action (G22): the silent-press / fire-on-release machine.
    Button(ButtonState),
    /// Cameo action (A2 / SelectClass): fire on the press edge (left build/arm,
    /// right cancel); strip LEFTUP; post `id|0x8000` (`|0x4000` marks a right
    /// press) for the driver to map back to a SidebarAction. Not sticky, no
    /// toggle/latch state.
    Cameo,
    /// Invisible Action-only click region (A3 / tactical catcher + minimap):
    /// consume any masked flags, run sticky capture, post NO id. The driver
    /// identifies which region consumed by stored handle.
    ClickRegion,
}

/// One retained gadget.
#[derive(Debug, Clone)]
pub struct Gadget {
    pub handle: GadgetHandle,
    pub rect: GadgetRect,
    /// Event mask (G15 filters raw flags by this FIRST). Bit 0x100 doubles as
    /// the keyboard-focus marker (G18).
    pub flags: u16,
    /// Result-protocol id; 0 posts nothing (G13).
    pub id: u16,
    pub is_sticky: bool,
    pub is_disabled: bool,
    /// Local dirty byte (G19): set by redraw-flag setters, cleared by draw.
    pub is_to_redraw: bool,
    pub behavior: GadgetBehavior,
}

/// Construction spec (the ctor argument set, G4).
#[derive(Debug, Clone, Copy)]
pub struct GadgetSpec {
    pub rect: GadgetRect,
    pub flags: u16,
    pub id: u16,
    pub sticky: bool,
    pub disabled: bool,
    pub behavior: GadgetBehavior,
}

impl GadgetSpec {
    /// G4 — base ctor: geometry + mask + sticky; `sticky ⇒ Flags |= 0x05`;
    /// everything else zeroed.
    pub fn new(rect: GadgetRect, flags: u16, sticky: bool) -> Self {
        let flags = if sticky {
            flags | STICKY_CTOR_MASK
        } else {
            flags
        };
        Self {
            rect,
            flags,
            id: 0,
            sticky,
            disabled: false,
            behavior: GadgetBehavior::Plain,
        }
    }

    /// Toggle-button ctor defaults (G4): mask 5 (left press+release), sticky.
    /// Callers override the mask afterwards (`with_flags`) for the 0x55
    /// scroll pair.
    pub fn button(rect: GadgetRect, id: u16, kind: ToggleKind) -> Self {
        let mut spec = Self::new(rect, 0x0005, true);
        spec.id = id;
        spec.behavior = GadgetBehavior::Button(ButtonState {
            is_pressed: false,
            is_on: false,
            kind,
        });
        spec
    }

    /// Replace the event mask AFTER ctor defaults (the native sidebar init
    /// writes the 0x55 scroll mask over the ctor's 5 — contract lane §2.1).
    pub fn with_flags(mut self, flags: u16) -> Self {
        self.flags = flags;
        self
    }

    /// SelectClass cameo ctor (A2): mask 0x19, NOT sticky, `Cameo` behavior.
    /// `id` is the runtime cameo id (1000 + visible slot index).
    pub fn cameo(rect: GadgetRect, id: u16) -> Self {
        let mut spec = Self::new(rect, super::CAMEO_FLAGS, false);
        spec.id = id;
        spec.behavior = GadgetBehavior::Cameo;
        spec
    }

    /// Invisible Action-only click region ctor (A3): sticky, custom event mask
    /// (tactical 0x7F / minimap 0x9F), `ClickRegion` behavior, no id. The sticky
    /// byte makes the dispatcher acquire capture on a press so a drag stays
    /// bound to the region across the sidebar boundary (G17). The 0x7F/0x9F
    /// masks already include the sticky `|5` bits.
    pub fn click_region(rect: GadgetRect, flags: u16) -> Self {
        let mut spec = Self::new(rect, flags, true);
        spec.behavior = GadgetBehavior::ClickRegion;
        spec
    }
}

#[derive(Debug)]
pub struct GadgetList {
    id: ListId,
    next_handle: u32,
    gadgets: Vec<Gadget>,
}

impl GadgetList {
    pub fn new(id: ListId) -> Self {
        Self {
            id,
            next_handle: 1,
            gadgets: Vec::new(),
        }
    }

    pub fn list_id(&self) -> ListId {
        self.id
    }

    pub fn len(&self) -> usize {
        self.gadgets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.gadgets.is_empty()
    }

    fn alloc(&mut self, spec: GadgetSpec) -> Gadget {
        let handle = GadgetHandle(self.next_handle);
        self.next_handle += 1;
        Gadget {
            handle,
            rect: spec.rect,
            flags: spec.flags,
            id: spec.id,
            is_sticky: spec.sticky,
            is_disabled: spec.disabled,
            is_to_redraw: false,
            behavior: spec.behavior,
        }
    }

    /// G2 — Add_Tail: append (the registration path, O7).
    pub fn add_tail(&mut self, spec: GadgetSpec) -> GadgetHandle {
        let g = self.alloc(spec);
        let h = g.handle;
        self.gadgets.push(g);
        h
    }

    /// G2 — Add_Head: prepend.
    #[cfg(test)]
    pub fn add_head(&mut self, spec: GadgetSpec) -> GadgetHandle {
        let g = self.alloc(spec);
        let h = g.handle;
        self.gadgets.insert(0, g);
        h
    }

    /// G2 — Add(after): insert immediately after an existing gadget.
    /// Returns None when `after` is not in this list.
    #[cfg(test)]
    pub fn add_after(&mut self, after: GadgetHandle, spec: GadgetSpec) -> Option<GadgetHandle> {
        let pos = self.gadgets.iter().position(|g| g.handle == after)?;
        let g = self.alloc(spec);
        let h = g.handle;
        self.gadgets.insert(pos + 1, g);
        Some(h)
    }

    /// G3 — Remove: neighbor repair is implicit in Vec removal; clears every
    /// focus slot pointing at the dying gadget (G18/G24 + hover closure).
    pub fn remove(&mut self, handle: GadgetHandle, focus: &mut FocusState) -> bool {
        let Some(pos) = self.gadgets.iter().position(|g| g.handle == handle) else {
            return false;
        };
        self.gadgets.remove(pos);
        focus.on_removed(handle);
        true
    }

    /// Extract_Gadget(id): remove the first gadget carrying a control id.
    #[cfg(test)]
    pub fn extract_by_id(&mut self, id: u16, focus: &mut FocusState) -> Option<GadgetHandle> {
        let handle = self.gadgets.iter().find(|g| g.id == id)?.handle;
        self.remove(handle, focus);
        Some(handle)
    }

    /// Delete_List: destroy every gadget, clearing focus slots per gadget (G24).
    pub fn clear(&mut self, focus: &mut FocusState) {
        for g in self.gadgets.drain(..) {
            focus.on_removed(g.handle);
        }
    }

    pub fn get(&self, handle: GadgetHandle) -> Option<&Gadget> {
        self.gadgets.iter().find(|g| g.handle == handle)
    }

    pub fn get_mut(&mut self, handle: GadgetHandle) -> Option<&mut Gadget> {
        self.gadgets.iter_mut().find(|g| g.handle == handle)
    }

    /// Handle at a retained-order index (tick walk helper; index < len).
    pub(crate) fn handle_at(&self, idx: usize) -> GadgetHandle {
        self.gadgets[idx].handle
    }

    /// Head→tail iteration in retained order.
    pub fn iter(&self) -> impl Iterator<Item = &Gadget> {
        self.gadgets.iter()
    }
}

/// G18 — focus acquire: steal keyboard focus. Old holder is dirtied and loses its
/// 0x100 mask bit; the new holder gains it.
#[cfg(test)]
pub fn set_focus(list: &mut GadgetList, focus: &mut FocusState, handle: GadgetHandle) {
    if let Some(old) = focus.keyboard.take()
        && let Some(g) = list.get_mut(old)
    {
        g.is_to_redraw = true;
        g.flags &= !FLAG_KEYBOARD;
    }
    if let Some(g) = list.get_mut(handle) {
        g.flags |= FLAG_KEYBOARD;
        focus.keyboard = Some(handle);
    }
}

/// G18 — focus clear: self-conditional (only the holder clears itself).
pub fn clear_focus(list: &mut GadgetList, focus: &mut FocusState, handle: GadgetHandle) {
    if focus.keyboard == Some(handle) {
        if let Some(g) = list.get_mut(handle) {
            g.is_to_redraw = true;
            g.flags &= !FLAG_KEYBOARD;
        }
        focus.keyboard = None;
    }
}

/// G18/G19 — enable/disable: set the gate, dirty unconditionally, force the
/// G18 focus clear.
pub fn set_enabled(
    list: &mut GadgetList,
    focus: &mut FocusState,
    handle: GadgetHandle,
    enabled: bool,
) {
    clear_focus(list, focus, handle);
    if let Some(g) = list.get_mut(handle) {
        g.is_disabled = !enabled;
        g.is_to_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> GadgetRect {
        GadgetRect::new(0, 0, 10, 10)
    }

    #[test]
    fn cameo_and_click_region_ctors_a2_a3() {
        let r = GadgetRect::new(0, 0, 60, 48);
        let cameo = GadgetSpec::cameo(r, 1000);
        assert!(!cameo.sticky, "cameo is NOT sticky");
        assert_eq!(cameo.flags, super::super::CAMEO_FLAGS, "cameo mask 0x19");
        assert_eq!(cameo.id, 1000);
        assert!(matches!(cameo.behavior, GadgetBehavior::Cameo));

        let region = GadgetSpec::click_region(GadgetRect::new(0, 0, 800, 600), 0x7F);
        assert!(region.sticky, "click region is sticky");
        assert_eq!(
            region.flags, 0x7F,
            "0x7F already includes the sticky |5 bits"
        );
        assert_eq!(region.id, 0, "no id");
        assert!(matches!(region.behavior, GadgetBehavior::ClickRegion));
    }

    #[test]
    fn ctor_defaults_g4() {
        let plain = GadgetSpec::new(rect(), 0x40, false);
        assert_eq!(plain.flags, 0x40, "non-sticky keeps the given mask");
        let sticky = GadgetSpec::new(rect(), 0x40, true);
        assert_eq!(sticky.flags, 0x45, "sticky ORs 0x05 into the mask (G4)");
        let btn = GadgetSpec::button(rect(), 0x65, ToggleKind::Flip);
        assert_eq!(btn.flags, 0x0005, "button ctor mask 5 (G4)");
        assert!(btn.sticky, "button ctor sticky (G4)");
        let scroll = GadgetSpec::button(rect(), 0xC9, ToggleKind::Plain).with_flags(0x55);
        assert_eq!(scroll.flags, 0x55, "sidebar init overrides the scroll mask");
    }

    #[test]
    fn retained_order_add_tail_head_after() {
        let mut f = FocusState::new();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::new(rect(), 0, false));
        let b = l.add_tail(GadgetSpec::new(rect(), 0, false));
        let c = l.add_head(GadgetSpec::new(rect(), 0, false));
        let d = l.add_after(a, GadgetSpec::new(rect(), 0, false)).unwrap();
        let order: Vec<GadgetHandle> = l.iter().map(|g| g.handle).collect();
        assert_eq!(order, vec![c, a, d, b], "head, then a, after-a, tail");
        assert!(l.remove(a, &mut f));
        assert!(!l.remove(a, &mut f), "double remove rejected");
        let order: Vec<GadgetHandle> = l.iter().map(|g| g.handle).collect();
        assert_eq!(order, vec![c, d, b]);
    }

    #[test]
    fn add_after_missing_returns_none() {
        let mut l = GadgetList::new(ListId(1));
        assert!(
            l.add_after(GadgetHandle(99), GadgetSpec::new(rect(), 0, false))
                .is_none()
        );
    }

    #[test]
    fn remove_clears_focus_slots_g24() {
        let mut f = FocusState::new();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::new(rect(), 0x05, true));
        f.sticky = Some(a);
        f.hovered = Some(a);
        set_focus(&mut l, &mut f, a);
        assert_eq!(f.keyboard, Some(a));
        l.remove(a, &mut f);
        assert_eq!(f.sticky, None);
        assert_eq!(f.keyboard, None);
        assert_eq!(f.hovered, None, "hover cleared — no Leave on a dead handle");
    }

    #[test]
    fn set_focus_steals_and_moves_keyboard_bit_g18() {
        let mut f = FocusState::new();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::new(rect(), 0, false));
        let b = l.add_tail(GadgetSpec::new(rect(), 0, false));
        set_focus(&mut l, &mut f, a);
        assert_eq!(l.get(a).unwrap().flags & FLAG_KEYBOARD, FLAG_KEYBOARD);
        // Steal.
        l.get_mut(a).unwrap().is_to_redraw = false;
        set_focus(&mut l, &mut f, b);
        assert_eq!(f.keyboard, Some(b));
        let ga = l.get(a).unwrap();
        assert_eq!(
            ga.flags & FLAG_KEYBOARD,
            0,
            "old holder loses the 0x100 bit"
        );
        assert!(ga.is_to_redraw, "old holder redrawn");
        assert_eq!(l.get(b).unwrap().flags & FLAG_KEYBOARD, FLAG_KEYBOARD);
        // The G18 focus clear is self-conditional.
        clear_focus(&mut l, &mut f, a);
        assert_eq!(f.keyboard, Some(b), "non-holder clear is a no-op");
        clear_focus(&mut l, &mut f, b);
        assert_eq!(f.keyboard, None);
    }

    #[test]
    fn disable_forces_clear_focus_and_dirty_g18_g19() {
        let mut f = FocusState::new();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::new(rect(), 0, false));
        set_focus(&mut l, &mut f, a);
        l.get_mut(a).unwrap().is_to_redraw = false;
        set_enabled(&mut l, &mut f, a, false);
        assert_eq!(f.keyboard, None);
        let g = l.get(a).unwrap();
        assert!(g.is_disabled);
        assert!(g.is_to_redraw, "Enable/Disable dirty unconditionally");
    }

    #[test]
    fn extract_by_id_and_clear() {
        let mut f = FocusState::new();
        let mut l = GadgetList::new(ListId(1));
        let mut spec = GadgetSpec::new(rect(), 0, false);
        spec.id = 0x65;
        let a = l.add_tail(spec);
        l.add_tail(GadgetSpec::new(rect(), 0, false));
        assert_eq!(l.extract_by_id(0x65, &mut f), Some(a));
        assert_eq!(l.extract_by_id(0x65, &mut f), None);
        f.sticky = l.iter().next().map(|g| g.handle);
        l.clear(&mut f);
        assert!(l.is_empty());
        assert_eq!(f.sticky, None, "clear releases per-gadget focus slots");
    }
}
