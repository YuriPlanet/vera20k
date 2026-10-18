//! The per-tick dispatch authority: hit-test (G14), per-gadget filter (G15),
//! and the three-tier event tick (G5-G13). Deterministic: all inputs arrive in
//! `GadgetInput`; NO wall-clock access.

use super::button::dispatch_action;
use super::focus::FocusState;
use super::list::GadgetList;
use super::{
    FLAG_KEYBOARD, FLAG_LEFT_HELD, FLAG_LEFT_PRESS, FLAG_LEFT_RELEASE, FLAG_LEFT_UP,
    FLAG_RIGHT_HELD, FLAG_RIGHT_PRESS, FLAG_RIGHT_RELEASE, FLAG_RIGHT_UP, GadgetHandle,
    HIT_SEED_AREA, KEY_LMB_DOWN, KEY_LMB_UP, KEY_RMB_DOWN, KEY_RMB_UP,
};

/// One tick's input snapshot, built by the app driver.
#[derive(Debug, Clone, Copy, Default)]
pub struct GadgetInput {
    /// Queued event: 0 = idle tick; KEY_* mouse codes; any other non-zero
    /// value = keyboard event (G8).
    pub queued_key: u16,
    /// Coordinates latched when the event was queued (G6: used iff the key's
    /// low byte is 1 or 2).
    pub event_x: i32,
    pub event_y: i32,
    /// Live cursor position (G6 idle/keyboard source; G22 inside-test source).
    pub mouse_x: i32,
    pub mouse_y: i32,
    /// Live button state (G8 held bits — idle ticks only).
    pub left_held: bool,
    pub right_held: bool,
    /// Modifier keys, polled fresh (G9).
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// One emitted paint (G11/G12/G19 cadence record).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawCmd {
    pub handle: GadgetHandle,
    pub forced: bool,
}

/// One Action dispatch (post-G15-masking) — test/observability record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchRecord {
    pub handle: GadgetHandle,
    pub masked_flags: u16,
    /// G9 modifier word: SHIFT=1 | CTRL=2 | ALT=4 (0 outside the broadcast tier).
    pub modifier: u8,
    /// Actual key after Action (button releases rewrite this to ID|0x8000).
    pub key_after: u16,
    /// Action's original return; nonzero means consumed, not a button ID.
    pub action_result: u32,
}

/// Tick results. Owned by the driver and reused across ticks (buffers are
/// cleared, not reallocated — no per-frame allocation in the input path).
#[derive(Debug, Default)]
pub struct TickOutput {
    pub draws: Vec<DrawCmd>,
    pub dispatches: Vec<DispatchRecord>,
    /// Hover transition this tick (G7): old gadget left / new gadget entered.
    pub hover_left: Option<GadgetHandle>,
    pub hover_entered: Option<GadgetHandle>,
    /// The broadcast-walk consumer (None for sticky/keyboard-tier ticks).
    pub consumed_by: Option<GadgetHandle>,
}

impl TickOutput {
    pub fn clear(&mut self) {
        self.draws.clear();
        self.dispatches.clear();
        self.hover_left = None;
        self.hover_entered = None;
        self.consumed_by = None;
    }
}

/// G14 — hit test: forward walk, skip disabled, HALF-OPEN rects, smallest
/// area wins with a signed `<=` tie-break (equal area ⇒ the LATER gadget
/// wins), seeded with the fixed 786,432 px² constant — a gadget larger than
/// the seed can never win.
pub fn hit_test(list: &GadgetList, mx: i32, my: i32) -> Option<GadgetHandle> {
    let mut best: Option<GadgetHandle> = None;
    let mut best_area: i32 = HIT_SEED_AREA;
    for g in list.iter() {
        if g.is_disabled || !g.rect.contains(mx, my) {
            continue;
        }
        let area = g.rect.area();
        if area <= best_area {
            best = Some(g.handle);
            best_area = area;
        }
    }
    best
}

/// G15 — clicked-on filter: mask FIRST, then dispatch iff the gadget is the sticky
/// holder (even masked-0), OR masked flags contain the keyboard bit (bounds
/// bypassed), OR masked flags are non-zero AND the point is inside the
/// half-open rect. Returns the Action result (non-zero = consumed).
pub(crate) fn clicked_on(
    list: &mut GadgetList,
    handle: GadgetHandle,
    key: &mut u16,
    raw_flags: u16,
    x: i32,
    y: i32,
    modifier: u8,
    live: (i32, i32),
    focus: &mut FocusState,
    out: &mut TickOutput,
) -> u32 {
    let is_sticky_holder = focus.sticky == Some(handle);
    let Some(g) = list.get_mut(handle) else {
        return 0;
    };
    let masked = raw_flags & g.flags;
    if !is_sticky_holder && (masked & FLAG_KEYBOARD) == 0 && (masked == 0 || !g.rect.contains(x, y))
    {
        return 0;
    }
    let action_result = dispatch_action(g, masked, key, live, focus);
    out.dispatches.push(DispatchRecord {
        handle,
        masked_flags: masked,
        modifier,
        key_after: *key,
        action_result,
    });
    action_result
}

/// G19 — draw(forced): paints iff forced or dirty, then clears the dirty
/// byte; the paint is emitted as a `DrawCmd`.
pub(crate) fn draw_one(
    list: &mut GadgetList,
    handle: GadgetHandle,
    forced: bool,
    out: &mut TickOutput,
) {
    if let Some(g) = list.get_mut(handle)
        && (forced || g.is_to_redraw)
    {
        g.is_to_redraw = false;
        out.draws.push(DrawCmd { handle, forced });
    }
}

/// One Input tick on a list (the G5-G13 dispatch authority). Returns the
/// 16-bit key, possibly rewritten to `ID|0x8000[|0x4000]` by a fired control
/// (G13) or forced to 0 by a silent press (G22 row 1).
pub fn tick(
    list: &mut GadgetList,
    focus: &mut FocusState,
    input: &GadgetInput,
    out: &mut TickOutput,
) -> u16 {
    out.clear();

    // G5 — fresh-list reset: a different list than last tick clears capture
    // and keyboard focus and force-draws every gadget this tick.
    let list_changed = focus.current_list != Some(list.list_id());
    if list_changed {
        focus.sticky = None;
        focus.keyboard = None;
        focus.current_list = Some(list.list_id());
    }

    let mut key: u16 = input.queued_key;

    // G6 — coordinate source: mouse-button events (low byte 1/2 — covers
    // 0x001/0x002/0x801/0x802) use the latched event coords; keyboard events
    // and idle ticks use the live cursor.
    let (x, y) = if matches!(key & 0xFF, 1 | 2) {
        (input.event_x, input.event_y)
    } else {
        (input.mouse_x, input.mouse_y)
    };

    // G7 — hover transitions run BEFORE dispatch, every tick.
    let hit = hit_test(list, x, y);
    if hit != focus.hovered {
        out.hover_left = focus.hovered;
        out.hover_entered = hit;
        focus.hovered = hit;
    }

    // G8 — flag assembly: event bits from the queued key; held/up bits ONLY
    // on idle ticks; a queued non-mouse event yields exactly FLAG_KEYBOARD.
    let mut flags: u16 = match key {
        0 => 0,
        KEY_LMB_DOWN => FLAG_LEFT_PRESS,
        KEY_RMB_DOWN => FLAG_RIGHT_PRESS,
        KEY_LMB_UP => FLAG_LEFT_RELEASE,
        KEY_RMB_UP => FLAG_RIGHT_RELEASE,
        _ => 0,
    };
    if key == 0 {
        flags |= if input.left_held {
            FLAG_LEFT_HELD
        } else {
            FLAG_LEFT_UP
        };
        flags |= if input.right_held {
            FLAG_RIGHT_HELD
        } else {
            FLAG_RIGHT_UP
        };
    } else if flags == 0 {
        flags = FLAG_KEYBOARD;
    }

    // G9 — modifier word, polled fresh; passed ONLY to the broadcast walk
    // (hardwired 0 for the sticky and keyboard tiers).
    let modifier: u8 =
        u8::from(input.shift) | (u8::from(input.ctrl) << 1) | (u8::from(input.alt) << 2);

    let live = (input.mouse_x, input.mouse_y);

    // G10 tier 1 — sticky capture: exclusive; dispatched even masked-0.
    if let Some(handle) = focus.sticky {
        if list.get(handle).is_some() {
            draw_one(list, handle, false, out); // G11 pre-draw
            clicked_on(list, handle, &mut key, flags, x, y, 0, live, focus, out);
            // G11 post-draw re-reads the capture slot: a gadget that released
            // capture this call still gets its post-draw.
            let post = focus.sticky.unwrap_or(handle);
            draw_one(list, post, false, out);
            return key;
        }
        // Unreachable by construction (removal clears focus); never dispatch
        // into a missing slot.
        focus.sticky = None;
    }

    // G10 tier 2 — keyboard focus: only for keyboard-flag ticks.
    if let Some(handle) = focus.keyboard
        && (flags & FLAG_KEYBOARD) != 0
        && list.get(handle).is_some()
    {
        draw_one(list, handle, false, out);
        clicked_on(list, handle, &mut key, flags, x, y, 0, live, focus, out);
        let post = focus.keyboard.unwrap_or(handle);
        draw_one(list, post, false, out);
        return key;
    }

    // G12 tier 3 — broadcast walk head→tail: every visited gadget is drawn
    // (forced on a fresh list) BEFORE dispatch; disabled gadgets are drawn
    // but never dispatched; the first consumer gets one extra draw and stops
    // the walk — later gadgets get NEITHER call this tick.
    for i in 0..list.len() {
        let handle = list.handle_at(i);
        draw_one(list, handle, list_changed, out);
        let disabled = list.get(handle).is_none_or(|g| g.is_disabled);
        if !disabled
            && clicked_on(
                list, handle, &mut key, flags, x, y, modifier, live, focus, out,
            ) != 0
        {
            draw_one(list, handle, false, out);
            out.consumed_by = Some(handle);
            break;
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::gadget::list::{GadgetBehavior, GadgetSpec};
    use crate::ui::gadget::{GadgetRect, ListId};

    fn spec(rect: GadgetRect, flags: u16) -> GadgetSpec {
        GadgetSpec::new(rect, flags, false)
    }

    #[test]
    fn g14_smallest_area_wins() {
        let mut l = GadgetList::new(ListId(1));
        let big = l.add_tail(spec(GadgetRect::new(0, 0, 100, 100), 0xFF));
        let small = l.add_tail(spec(GadgetRect::new(10, 10, 20, 20), 0xFF));
        assert_eq!(
            hit_test(&l, 15, 15),
            Some(small),
            "smaller wins inside both"
        );
        assert_eq!(hit_test(&l, 90, 90), Some(big), "only the big one contains");
        assert_eq!(hit_test(&l, 200, 200), None);
    }

    #[test]
    fn g14_equal_area_later_wins() {
        let mut l = GadgetList::new(ListId(1));
        let _first = l.add_tail(spec(GadgetRect::new(0, 0, 20, 20), 0xFF));
        let second = l.add_tail(spec(GadgetRect::new(0, 0, 20, 20), 0xFF));
        assert_eq!(
            hit_test(&l, 5, 5),
            Some(second),
            "signed <= tie-break: later-in-list wins on equal area"
        );
    }

    #[test]
    fn g14_disabled_invisible_and_seed_caps_area() {
        let mut l = GadgetList::new(ListId(1));
        let mut d = spec(GadgetRect::new(0, 0, 10, 10), 0xFF);
        d.disabled = true;
        l.add_tail(d);
        assert_eq!(hit_test(&l, 5, 5), None, "disabled gadgets are invisible");
        // Area 1024*768 ties the seed (signed <=) and CAN win; one px more cannot.
        let mut l2 = GadgetList::new(ListId(2));
        let exact = l2.add_tail(spec(GadgetRect::new(0, 0, 1024, 768), 0xFF));
        assert_eq!(hit_test(&l2, 5, 5), Some(exact), "area == seed ties via <=");
        let mut l3 = GadgetList::new(ListId(3));
        l3.add_tail(spec(GadgetRect::new(0, 0, 1024, 769), 0xFF));
        assert_eq!(hit_test(&l3, 5, 5), None, "area > 786,432 can never win");
    }

    #[test]
    fn g14_half_open_boundary() {
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(spec(GadgetRect::new(10, 10, 5, 5), 0xFF));
        assert_eq!(hit_test(&l, 10, 10), Some(a));
        assert_eq!(hit_test(&l, 14, 14), Some(a));
        assert_eq!(hit_test(&l, 15, 10), None, "right edge out");
        assert_eq!(hit_test(&l, 10, 15), None, "bottom edge out");
    }

    #[test]
    fn g15_mask_first_filter() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let mut s = spec(GadgetRect::new(0, 0, 10, 10), 0x05);
        s.id = 0x65;
        s.behavior = GadgetBehavior::Control;
        let a = l.add_tail(s);
        let mut key: u16 = 0;
        // Raw flags 0x40 (right release) mask to 0 against 0x05 → early-out
        // even though the point is inside.
        assert_eq!(
            clicked_on(&mut l, a, &mut key, 0x40, 5, 5, 0, (5, 5), &mut f, &mut out),
            0
        );
        assert!(out.dispatches.is_empty(), "filtered before dispatch");
        // Masked non-zero but outside the rect → early-out.
        assert_eq!(
            clicked_on(
                &mut l,
                a,
                &mut key,
                0x01,
                50,
                50,
                0,
                (50, 50),
                &mut f,
                &mut out
            ),
            0
        );
        // Masked non-zero and inside → dispatches.
        assert_eq!(
            clicked_on(&mut l, a, &mut key, 0x01, 5, 5, 0, (5, 5), &mut f, &mut out),
            1
        );
        assert_eq!(out.dispatches.len(), 1);
        assert_eq!(out.dispatches[0].masked_flags, 0x01);
    }

    #[test]
    fn g15_sticky_holder_bypasses_even_masked_0() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::button(
            GadgetRect::new(0, 0, 10, 10),
            0x65,
            crate::ui::gadget::list::ToggleKind::Flip,
        ));
        f.sticky = Some(a);
        let mut key: u16 = 0;
        // Raw held flags mask to 0 against mask 5; the holder is dispatched
        // anyway (masked-0 hover-track path).
        clicked_on(
            &mut l,
            a,
            &mut key,
            0x82,
            50,
            50,
            0,
            (50, 50),
            &mut f,
            &mut out,
        );
        assert_eq!(out.dispatches.len(), 1);
        assert_eq!(out.dispatches[0].masked_flags, 0);
    }

    #[test]
    fn g15_keyboard_flag_bypasses_bounds() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let mut s = spec(GadgetRect::new(0, 0, 10, 10), 0x05 | FLAG_KEYBOARD);
        s.id = 0x42;
        s.behavior = GadgetBehavior::Control;
        let a = l.add_tail(s);
        let mut key: u16 = 0;
        // Keyboard flag, point far outside → still dispatches.
        assert_eq!(
            clicked_on(
                &mut l,
                a,
                &mut key,
                FLAG_KEYBOARD,
                500,
                500,
                0,
                (500, 500),
                &mut f,
                &mut out
            ),
            1
        );
        assert_eq!(key, 0x42 | 0x8000);
    }

    #[test]
    fn draw_one_dirty_gate_g19() {
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(spec(GadgetRect::new(0, 0, 10, 10), 0));
        draw_one(&mut l, a, false, &mut out);
        assert!(out.draws.is_empty(), "clean + unforced = no paint");
        l.get_mut(a).unwrap().is_to_redraw = true;
        draw_one(&mut l, a, false, &mut out);
        assert_eq!(out.draws.len(), 1);
        assert!(
            !l.get(a).unwrap().is_to_redraw,
            "paint clears the dirty byte"
        );
        draw_one(&mut l, a, true, &mut out);
        assert_eq!(out.draws.len(), 2, "forced always paints");
    }

    fn btn(rect: GadgetRect, id: u16) -> GadgetSpec {
        GadgetSpec::button(rect, id, crate::ui::gadget::list::ToggleKind::Flip)
    }

    fn idle(mx: i32, my: i32) -> GadgetInput {
        GadgetInput {
            mouse_x: mx,
            mouse_y: my,
            ..Default::default()
        }
    }

    fn event(key: u16, ex: i32, ey: i32, held_left: bool) -> GadgetInput {
        GadgetInput {
            queued_key: key,
            event_x: ex,
            event_y: ey,
            mouse_x: ex,
            mouse_y: ey,
            left_held: held_left,
            ..Default::default()
        }
    }

    #[test]
    fn g5_fresh_list_reset_clears_capture_and_force_draws() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l1 = GadgetList::new(ListId(1));
        let a = l1.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        // Press captures on list 1.
        tick(
            &mut l1,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(f.sticky, Some(a));
        // Ticking a DIFFERENT list resets capture + keyboard, force-draws all.
        let mut l2 = GadgetList::new(ListId(2));
        l2.add_tail(GadgetSpec::new(GadgetRect::new(0, 0, 5, 5), 0, false));
        l2.add_tail(GadgetSpec::new(GadgetRect::new(0, 0, 5, 5), 0, false));
        tick(&mut l2, &mut f, &idle(100, 100), &mut out);
        assert_eq!(f.sticky, None, "G5 nulls capture");
        assert_eq!(f.current_list, Some(ListId(2)));
        assert_eq!(out.draws.len(), 2, "fresh list force-draws every gadget");
        assert!(out.draws.iter().all(|d| d.forced));
    }

    #[test]
    fn g6_event_coords_for_mouse_keys_live_for_idle() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        // Event coords inside the rect, live mouse far away: the press (low
        // byte 1) must hit-test/dispatch at the EVENT coords.
        let mut input = event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true);
        input.mouse_x = 500;
        input.mouse_y = 500;
        tick(&mut l, &mut f, &input, &mut out);
        assert_eq!(f.sticky, Some(a), "dispatched at event coords");
        assert_eq!(f.hovered, Some(a), "hover hit-tested at event coords too");
        // Idle tick uses the live cursor: far away ⇒ hover leaves.
        let mut f2 = FocusState::new();
        f2.current_list = Some(ListId(1));
        f2.hovered = Some(a);
        tick(&mut l, &mut f2, &idle(500, 500), &mut out);
        assert_eq!(f2.hovered, None);
        assert_eq!(out.hover_left, Some(a));
    }

    #[test]
    fn g7_hover_enter_leave_and_removal_closure() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::new(GadgetRect::new(0, 0, 10, 10), 0, false));
        tick(&mut l, &mut f, &idle(5, 5), &mut out);
        assert_eq!(out.hover_entered, Some(a));
        assert_eq!(f.hovered, Some(a));
        // Removing the hovered gadget clears hover; the NEXT tick reports no
        // hover_left for the dead handle (study §6.1 G7-closure).
        l.remove(a, &mut f);
        assert_eq!(f.hovered, None);
        tick(&mut l, &mut f, &idle(5, 5), &mut out);
        assert_eq!(out.hover_left, None, "no Leave fires for a dead handle");
        assert_eq!(out.hover_entered, None);
    }

    #[test]
    fn g8_flag_assembly_held_bits_idle_only() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        // Mask everything so the dispatch record shows the assembled flags.
        let mut s = GadgetSpec::new(GadgetRect::new(0, 0, 10, 10), 0x01FF, false);
        s.behavior = GadgetBehavior::Control;
        s.id = 0x11;
        l.add_tail(s);
        // Idle tick, left held, right up → 0x2 | 0x80.
        let mut input = idle(5, 5);
        input.left_held = true;
        tick(&mut l, &mut f, &input, &mut out);
        assert_eq!(
            out.dispatches[0].masked_flags,
            FLAG_LEFT_HELD | FLAG_RIGHT_UP
        );
        // Press event tick with left ALSO held: event bit only, NO held bits.
        let input = event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true);
        tick(&mut l, &mut f, &input, &mut out);
        assert_eq!(
            out.dispatches[0].masked_flags, FLAG_LEFT_PRESS,
            "never both"
        );
        // Queued non-mouse key → exactly FLAG_KEYBOARD.
        let input = event(0x1C, 5, 5, false);
        tick(&mut l, &mut f, &input, &mut out);
        assert_eq!(out.dispatches[0].masked_flags, FLAG_KEYBOARD);
    }

    #[test]
    fn g9_modifier_word_broadcast_only() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        let mut input = event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true);
        input.shift = true;
        input.alt = true;
        // Broadcast-tier dispatch carries the modifier word.
        tick(&mut l, &mut f, &input, &mut out);
        assert_eq!(out.dispatches[0].modifier, 0b101, "SHIFT=1 | ALT=4");
        assert_eq!(f.sticky, Some(a));
        // Sticky-tier re-dispatch hardwires 0.
        let mut input2 = idle(5, 5);
        input2.left_held = true;
        input2.shift = true;
        tick(&mut l, &mut f, &input2, &mut out);
        assert_eq!(out.dispatches[0].modifier, 0, "sticky tier modifier = 0");
    }

    #[test]
    fn g10_tier_precedence_sticky_exclusive() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        let b = l.add_tail(btn(GadgetRect::new(20, 0, 10, 10), 0x66));
        // Capture a.
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(f.sticky, Some(a));
        // A press over b while a holds capture goes to a ONLY (tier 1 is
        // exclusive); b is never dispatched.
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 25, 5, true),
            &mut out,
        );
        assert_eq!(out.dispatches.len(), 1);
        assert_eq!(out.dispatches[0].handle, a);
        assert_eq!(out.consumed_by, None, "no broadcast walk ran");
        let _ = b;
    }

    #[test]
    fn g10_keyboard_tier_and_g13_result() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let mut s = GadgetSpec::new(GadgetRect::new(0, 0, 10, 10), 0x05, false);
        s.id = 0x42;
        s.behavior = GadgetBehavior::Control;
        let a = l.add_tail(s);
        crate::ui::gadget::list::set_focus(&mut l, &mut f, a);
        f.current_list = Some(ListId(1));
        // Keyboard event with the cursor far away: routed to the focus
        // holder, bounds bypassed, result = ID|0x8000.
        let result = tick(&mut l, &mut f, &event(0x1C, 500, 500, false), &mut out);
        assert_eq!(result, 0x42 | 0x8000);
        assert_eq!(out.dispatches[0].handle, a);
        // A MOUSE event does not enter the keyboard tier (falls to broadcast,
        // misses the rect, returns the raw key).
        let result = tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 500, 500, false),
            &mut out,
        );
        assert_eq!(result, crate::ui::gadget::KEY_LMB_DOWN);
    }

    #[test]
    fn g12_walk_stops_at_consumer_draw_cadence() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        let b = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x66)); // same rect, later
        let c = l.add_tail(btn(GadgetRect::new(40, 0, 10, 10), 0x67));
        // Prime current_list so this is NOT a fresh tick.
        tick(&mut l, &mut f, &idle(100, 100), &mut out);
        // Press inside a+b: the walk visits a (clicked_on consumes — a is
        // FIRST in walk order; note hit-test priority would pick b, but the
        // broadcast walk dispatches in LIST order and a consumes first).
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(out.consumed_by, Some(a), "walk order, first consumer stops");
        // a got visited-draw + consumer-draw (both dirty-gated); c never
        // visited after the break: dispatch list has exactly one entry.
        assert_eq!(out.dispatches.len(), 1);
        let _ = (b, c);
    }

    #[test]
    fn g22_end_to_end_click_fires_on_release_only() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let _a = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        // Press: consumed, returns 0 (silent).
        let r = tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(r, 0, "silent press");
        // Idle held tick (sticky re-dispatch, masked-0): nothing fires.
        let mut held = idle(5, 5);
        held.left_held = true;
        let r = tick(&mut l, &mut f, &held, &mut out);
        assert_eq!(r, 0);
        // Release inside: fires ID|0x8000.
        let r = tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_UP, 5, 5, false),
            &mut out,
        );
        assert_eq!(r, 0x65 | 0x8000, "fire on release-inside");
        assert_eq!(f.sticky, None);
    }

    #[test]
    fn g22_end_to_end_drag_off_cancels() {
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let _a = l.add_tail(btn(GadgetRect::new(0, 0, 10, 10), 0x65));
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        // Drag off (idle tick, cursor outside).
        let mut held = idle(50, 50);
        held.left_held = true;
        tick(&mut l, &mut f, &held, &mut out);
        // Release outside: nothing fires.
        let mut up = event(crate::ui::gadget::KEY_LMB_UP, 50, 50, false);
        up.mouse_x = 50;
        up.mouse_y = 50;
        let r = tick(&mut l, &mut f, &up, &mut out);
        assert_eq!(
            r,
            crate::ui::gadget::KEY_LMB_UP,
            "no result posted — cancelled"
        );
        assert_eq!(f.sticky, None, "capture released");
    }

    #[test]
    fn cameo_fires_on_press_only_a2() {
        use crate::ui::gadget::CAMEO_FLAGS;
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let mut s = GadgetSpec::new(GadgetRect::new(0, 0, 60, 48), CAMEO_FLAGS, false);
        s.id = 1000;
        s.behavior = GadgetBehavior::Cameo;
        let a = l.add_tail(s);
        // Prime current_list (hover off the cameo).
        tick(&mut l, &mut f, &idle(500, 500), &mut out);
        // Left press inside → fires 1000|0x8000, consumes.
        let r = tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(r, 1000 | 0x8000);
        assert_eq!(out.consumed_by, Some(a));
        assert_eq!(f.sticky, None, "cameo never captures (not sticky)");
        // Idle tick over the cameo (left up): LEFTUP stripped → no fire, walk
        // not consumed.
        let r = tick(&mut l, &mut f, &idle(5, 5), &mut out);
        assert_eq!(r, 0);
        assert_eq!(out.consumed_by, None, "idle-over-cameo does not consume");
        assert_eq!(
            f.hovered,
            Some(a),
            "but it IS the current hover target (G7)"
        );
        // Release inside: cameo mask has no LEFTRELEASE → masked 0 → no dispatch.
        let r = tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_UP, 5, 5, false),
            &mut out,
        );
        assert_eq!(
            r,
            crate::ui::gadget::KEY_LMB_UP,
            "release not consumed by cameo"
        );
        assert_eq!(out.consumed_by, None);
    }

    #[test]
    fn a3_smaller_or_earlier_wins_overlapping_region_and_button() {
        // A small button overlapping a large catcher: the button consumes the
        // press first (earlier in the walk) — the catcher is the fallback only.
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let button = l.add_tail(btn(GadgetRect::new(0, 0, 20, 20), 0x65));
        let _catcher = l.add_tail(GadgetSpec::click_region(
            GadgetRect::new(0, 0, 800, 600),
            0x7F,
        ));
        tick(&mut l, &mut f, &idle(500, 500), &mut out); // prime
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(
            out.consumed_by,
            Some(button),
            "button (earlier) consumes, not the catcher"
        );
    }

    #[test]
    fn a3_catcher_dispatches_above_hit_seed_area() {
        // A region larger than HIT_SEED_AREA still consumes a contained press in
        // the broadcast walk (dispatch uses rect.contains, not the hover seed).
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let catcher = l.add_tail(GadgetSpec::click_region(
            GadgetRect::new(0, 0, 1920, 1080),
            0x7F,
        ));
        const { assert!(1920 * 1080 > crate::ui::gadget::HIT_SEED_AREA) };
        tick(&mut l, &mut f, &idle(5000, 5000), &mut out); // prime, hover off
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 100, 100, true),
            &mut out,
        );
        assert_eq!(
            out.consumed_by,
            Some(catcher),
            "rect.contains dispatch, seed-independent"
        );
        assert_eq!(f.sticky, Some(catcher), "press acquires sticky capture");
    }

    #[test]
    fn a3_sticky_region_keeps_drag_across_boundary() {
        // Press on the catcher captures; an idle masked-0 tick re-dispatches the
        // HOLDER even with the cursor over a (later) button rect (G15 bypass).
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let catcher = l.add_tail(GadgetSpec::click_region(
            GadgetRect::new(0, 0, 100, 100),
            0x7F,
        ));
        let _button = l.add_tail(btn(GadgetRect::new(200, 0, 20, 20), 0x65));
        tick(&mut l, &mut f, &idle(500, 500), &mut out);
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_LMB_DOWN, 5, 5, true),
            &mut out,
        );
        assert_eq!(f.sticky, Some(catcher));
        // Drag onto the button rect: held idle tick goes to the catcher (sticky
        // tier exclusive), the button is never dispatched.
        let mut held = idle(205, 5);
        held.left_held = true;
        tick(&mut l, &mut f, &held, &mut out);
        assert_eq!(out.dispatches.len(), 1);
        assert_eq!(
            out.dispatches[0].handle, catcher,
            "held drag stays with the catcher"
        );
        // Release over the button still releases the catcher's capture.
        let up = event(crate::ui::gadget::KEY_LMB_UP, 205, 5, false);
        tick(&mut l, &mut f, &up, &mut out);
        assert_eq!(f.sticky, None, "release frees capture");
    }

    #[test]
    fn minimap_right_click_releases_sticky_with_0xdf() {
        use crate::ui::gadget::MINIMAP_REGION_FLAGS;
        // The minimap region mask (0xDF) must let a right-press acquire AND a
        // right-release CLEAR sticky capture — else the region stays captured and
        // eats the next click.
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(1));
        let region = l.add_tail(GadgetSpec::click_region(
            GadgetRect::new(0, 0, 50, 50),
            MINIMAP_REGION_FLAGS,
        ));
        tick(&mut l, &mut f, &idle(500, 500), &mut out); // prime, hover off
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_RMB_DOWN, 5, 5, false),
            &mut out,
        );
        assert_eq!(f.sticky, Some(region), "right-press acquires capture");
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_RMB_UP, 5, 5, false),
            &mut out,
        );
        assert_eq!(
            f.sticky, None,
            "right-release frees capture (0xDF) — no stuck"
        );
    }

    #[test]
    fn minimap_literal_0x9f_would_leave_capture_stuck() {
        // Documents WHY we diverge to 0xDF: gamemd's literal 0x9F masks out
        // RIGHT_RELEASE, so a right-press captures and never releases — the
        // latent input-eating bug.
        let mut f = FocusState::new();
        let mut out = TickOutput::default();
        let mut l = GadgetList::new(ListId(2));
        let region = l.add_tail(GadgetSpec::click_region(
            GadgetRect::new(0, 0, 50, 50),
            0x9F,
        ));
        tick(&mut l, &mut f, &idle(500, 500), &mut out);
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_RMB_DOWN, 5, 5, false),
            &mut out,
        );
        assert_eq!(f.sticky, Some(region));
        tick(
            &mut l,
            &mut f,
            &event(crate::ui::gadget::KEY_RMB_UP, 5, 5, false),
            &mut out,
        );
        assert_eq!(
            f.sticky,
            Some(region),
            "0x9F leaves capture stuck — hence 0xDF"
        );
    }
}
