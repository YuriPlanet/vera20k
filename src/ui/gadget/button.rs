//! Per-gadget Action implementations: sticky capture protocol (G17), base
//! consume (G16), control result posting (G13), and the toggle-button
//! machine (G22/G23). Pure functions over `Gadget` + `FocusState`; the tick
//! (tick.rs) routes into `dispatch_action` after G15 filtering.

use super::focus::FocusState;
use super::list::{Gadget, GadgetBehavior, ToggleKind};
use super::{
    FLAG_LEFT_PRESS, FLAG_LEFT_UP, FLAG_RIGHT_PRESS, FLAG_RIGHT_RELEASE, PRESS_BITS, RELEASE_BITS,
    RESULT_BUTTON, RESULT_RIGHT,
};

/// G17 — sticky capture protocol: press bits acquire capture iff the gadget is sticky;
/// release bits release holder-only (an acquire+release in one call both run).
pub(crate) fn sticky_process(g: &Gadget, masked: u16, focus: &mut FocusState) {
    if g.is_sticky && (masked & PRESS_BITS) != 0 {
        focus.sticky = Some(g.handle);
    } else if focus.sticky != Some(g.handle) {
        return;
    }
    if (masked & RELEASE_BITS) != 0 {
        focus.sticky = None;
    }
}

/// G16 — base Action: masked-0 consumes nothing; anything else dirties the
/// gadget, runs the capture protocol, and consumes.
pub(crate) fn base_action(g: &mut Gadget, masked: u16, focus: &mut FocusState) -> u32 {
    if masked == 0 {
        return 0;
    }
    g.is_to_redraw = true;
    sticky_process(g, masked, focus);
    1
}

/// G13 — control action: post `id|0x8000` (`|0x4000` iff a right-release
/// fired AND the gadget's mask includes right-press), then chain to base.
/// (The live in-game population has no peer links — peer callbacks are not
/// modeled.)
pub(crate) fn control_action(
    g: &mut Gadget,
    masked: u16,
    key: &mut u16,
    focus: &mut FocusState,
) -> u32 {
    if masked != 0 {
        *key = if g.id == 0 { 0 } else { g.id | RESULT_BUTTON };
        if (masked & FLAG_RIGHT_RELEASE) != 0 && (g.flags & FLAG_RIGHT_PRESS) != 0 {
            *key = g.id | RESULT_BUTTON | RESULT_RIGHT;
        }
    }
    base_action(g, masked, focus)
}

/// SelectClass cameo Action (A2): fires on the press edge. gamemd strips the
/// LEFTUP bit and acts only on LEFTPRESS/RIGHTPRESS — so an idle-tick dispatch
/// over a hovered cameo (which carries LEFTUP) is a no-op that does NOT consume
/// (the walk continues). On a real press it posts `id|0x8000` (left) or
/// `id|0x8000|0x4000` (right marker) and consumes. Cameos are not sticky, so no
/// capture runs.
pub(crate) fn cameo_action(g: &mut Gadget, masked: u16, key: &mut u16) -> u32 {
    // gamemd: `if (flags & LEFTUP) flags &= ~LEFTUP;`
    let masked = masked & !FLAG_LEFT_UP;
    let press = masked & (FLAG_LEFT_PRESS | FLAG_RIGHT_PRESS);
    if press == 0 {
        // No press edge left after the LEFTUP strip → no fire, no consume.
        return 0;
    }
    if g.id != 0 {
        *key = g.id | RESULT_BUTTON;
        if (press & FLAG_RIGHT_PRESS) != 0 {
            *key |= RESULT_RIGHT; // driver reads this as right_click
        }
    }
    g.is_to_redraw = true;
    1
}

/// Behavior router called by `clicked_on` after G15 masking. `live` is the
/// LIVE cursor position (G22's inside-test source — never the event coords).
pub(crate) fn dispatch_action(
    g: &mut Gadget,
    masked: u16,
    key: &mut u16,
    live: (i32, i32),
    focus: &mut FocusState,
) -> u32 {
    match g.behavior {
        // ClickRegion (A3) is an invisible sticky region: base consume + capture,
        // no id post.
        GadgetBehavior::Plain | GadgetBehavior::ClickRegion => base_action(g, masked, focus),
        GadgetBehavior::Control => control_action(g, masked, key, focus),
        GadgetBehavior::Button(_) => toggle_action(g, masked, key, live, focus),
        GadgetBehavior::Cameo => cameo_action(g, masked, key),
    }
}

/// G22 — the toggle-button machine as the verified 7-row state table.
/// Preliminaries run on EVERY call in this order: (1) inside-test against the
/// LIVE cursor (half-open), (2) masked-0 hover-track (reachable only as the
/// sticky holder, G15), (3) capture acquire/release. Then the press / release
/// rows. Hold-repeat (G23) is purely the mask property: held bits fall
/// through to the tail control_action every tick — no timer, no delay.
fn toggle_action(
    g: &mut Gadget,
    masked: u16,
    key: &mut u16,
    live: (i32, i32),
    focus: &mut FocusState,
) -> u32 {
    let GadgetBehavior::Button(mut b) = g.behavior else {
        // dispatch_action only routes Button behaviors here.
        return 0;
    };
    // Step 1 — LIVE mouse inside-test, never the queued event coords.
    let inside = g.rect.contains(live.0, live.1);
    // Step 2 — rows 2/3: masked-0 sticky re-dispatch pops/restores is_pressed
    // (this is what cancels on drag-off and re-arms on drag-back).
    if masked == 0 {
        if inside && !b.is_pressed {
            b.is_pressed = true;
            g.is_to_redraw = true;
        } else if !inside && b.is_pressed {
            b.is_pressed = false;
            g.is_to_redraw = true;
        }
    }
    // Step 3 — capture protocol BEFORE the branch rows.
    sticky_process(g, masked, focus);

    // Row 1 — press: silent consume. Press bits are stripped from the tail
    // call (no ID posts unless other bits remain), then the key is FORCED to
    // 0 and the event is consumed.
    if (masked & PRESS_BITS) != 0 {
        b.is_pressed = true;
        g.is_to_redraw = true;
        g.behavior = GadgetBehavior::Button(b);
        control_action(g, masked & !PRESS_BITS, key, focus);
        *key = 0;
        return 1;
    }

    let mut tail_flags = masked;
    if (masked & RELEASE_BITS) != 0 {
        if b.is_pressed {
            // Rows 5/6 — release while pressed: toggle per Kind iff the LIVE
            // cursor is inside; release bits are KEPT so the tail posts
            // ID|0x8000 (G13). Release-outside still fires — reachable only
            // in the no-intervening-idle-tick boundary case (row 2 would have
            // popped is_pressed first).
            if inside {
                match b.kind {
                    ToggleKind::Flip => b.is_on = !b.is_on,
                    ToggleKind::LatchOn => b.is_on = true,
                    ToggleKind::Plain => {}
                }
            }
            b.is_pressed = false;
            g.is_to_redraw = true;
        } else {
            // Row 4 — release while NOT pressed (the drag-off cancel
            // outcome): strip the release bits; the tail fires nothing
            // unless other masked bits remain.
            tail_flags &= !RELEASE_BITS;
        }
    }
    g.behavior = GadgetBehavior::Button(b);
    // Row 7 — held bits (when masked in) reach the tail every tick (G23).
    control_action(g, tail_flags, key, focus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::gadget::list::{GadgetList, GadgetSpec};
    use crate::ui::gadget::{GadgetRect, ListId};

    fn one_gadget(spec: GadgetSpec) -> (GadgetList, crate::ui::gadget::GadgetHandle) {
        let mut l = GadgetList::new(ListId(1));
        let h = l.add_tail(spec);
        (l, h)
    }

    #[test]
    fn cameo_fires_on_press_strips_leftup_a2() {
        let mut spec = GadgetSpec::cameo(GadgetRect::new(0, 0, 60, 48), 1000);
        spec.behavior = GadgetBehavior::Cameo;
        let (mut l, a) = one_gadget(spec);
        let mut key: u16 = 0;
        // Left press → post id|0x8000, consume.
        assert_eq!(cameo_action(l.get_mut(a).unwrap(), 0x01, &mut key), 1);
        assert_eq!(key, 1000 | 0x8000);
        // Right press → post id|0x8000|0x4000, consume.
        key = 0;
        assert_eq!(cameo_action(l.get_mut(a).unwrap(), 0x10, &mut key), 1);
        assert_eq!(key, 1000 | 0x8000 | 0x4000);
        // LEFTUP only (idle-tick dispatch over a hovered cameo) → no fire, no
        // consume, key untouched.
        key = 0xBEEF;
        assert_eq!(cameo_action(l.get_mut(a).unwrap(), 0x08, &mut key), 0);
        assert_eq!(key, 0xBEEF);
    }

    #[test]
    fn cameo_zero_id_posts_nothing_but_consumes_a2() {
        let mut spec = GadgetSpec::cameo(GadgetRect::new(0, 0, 60, 48), 0);
        spec.behavior = GadgetBehavior::Cameo;
        let (mut l, a) = one_gadget(spec);
        let mut key: u16 = 0x1234;
        assert_eq!(cameo_action(l.get_mut(a).unwrap(), 0x01, &mut key), 1);
        assert_eq!(key, 0x1234, "id 0 posts nothing");
    }

    #[test]
    fn sticky_acquire_and_holder_only_release_g17() {
        let mut f = FocusState::new();
        // Same list so a/b carry DISTINCT handles (handles are per-list).
        let mut l = GadgetList::new(ListId(1));
        let a = l.add_tail(GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0x45, true));
        let b = l.add_tail(GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0x45, true));
        // Press acquires.
        sticky_process(l.get(a).unwrap(), 0x01, &mut f);
        assert_eq!(f.sticky, Some(a));
        // A non-holder's release does NOT release.
        sticky_process(l.get(b).unwrap(), 0x04, &mut f);
        assert_eq!(f.sticky, Some(a), "holder-only release");
        // Holder's release releases.
        sticky_process(l.get(a).unwrap(), 0x04, &mut f);
        assert_eq!(f.sticky, None);
        // Press+release in one call acquires then releases.
        sticky_process(l.get(a).unwrap(), 0x05, &mut f);
        assert_eq!(f.sticky, None);
        // Non-sticky gadget never acquires.
        let (l3, c) = one_gadget(GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0x45, false));
        sticky_process(l3.get(c).unwrap(), 0x01, &mut f);
        assert_eq!(f.sticky, None);
        let _ = c;
    }

    #[test]
    fn base_action_g16() {
        let mut f = FocusState::new();
        let (mut l, a) = one_gadget(GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0xFF, true));
        let g = l.get_mut(a).unwrap();
        assert_eq!(base_action(g, 0, &mut f), 0, "masked-0 consumes nothing");
        assert!(!g.is_to_redraw);
        assert_eq!(base_action(g, 0x08, &mut f), 1, "any masked bits consume");
        assert!(g.is_to_redraw);
    }

    #[test]
    fn control_action_result_protocol_g13() {
        let mut f = FocusState::new();
        // Mask includes right press (0x10) → right-release posts |0x4000.
        let mut spec = GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0x55, true);
        spec.id = 0xC9;
        spec.behavior = GadgetBehavior::Control;
        let (mut l, a) = one_gadget(spec);
        let mut key: u16 = 0;
        let g = l.get_mut(a).unwrap();
        assert_eq!(control_action(g, 0x04, &mut key, &mut f), 1);
        assert_eq!(key, 0xC9 | 0x8000, "left release posts ID|0x8000");
        key = 0;
        assert_eq!(control_action(g, 0x40, &mut key, &mut f), 1);
        assert_eq!(
            key,
            0xC9 | 0xC000,
            "right release + masked 0x10 posts ID|0xC000"
        );
        // Mask WITHOUT right press: right-release does not add 0x4000.
        let mut spec2 = GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0x45, true);
        spec2.id = 0x65;
        spec2.behavior = GadgetBehavior::Control;
        let (mut l2, b) = one_gadget(spec2);
        key = 0;
        assert_eq!(
            control_action(l2.get_mut(b).unwrap(), 0x40, &mut key, &mut f),
            1
        );
        assert_eq!(key, 0x65 | 0x8000);
        // id 0 posts 0 on the plain branch.
        let mut spec3 = GadgetSpec::new(GadgetRect::new(0, 0, 4, 4), 0x45, true);
        spec3.behavior = GadgetBehavior::Control;
        let (mut l3, c) = one_gadget(spec3);
        key = 0x1234;
        assert_eq!(
            control_action(l3.get_mut(c).unwrap(), 0x04, &mut key, &mut f),
            1
        );
        assert_eq!(key, 0, "ID==0 posts 0");
        // masked-0 leaves the key untouched and consumes nothing.
        key = 0x1234;
        assert_eq!(
            control_action(l3.get_mut(c).unwrap(), 0, &mut key, &mut f),
            0
        );
        assert_eq!(key, 0x1234);
    }

    fn button(
        id: u16,
        kind: ToggleKind,
        flags: u16,
    ) -> (GadgetList, crate::ui::gadget::GadgetHandle) {
        let spec = GadgetSpec::button(GadgetRect::new(0, 0, 10, 10), id, kind).with_flags(flags);
        one_gadget(spec)
    }

    const INSIDE: (i32, i32) = (5, 5);
    const OUTSIDE: (i32, i32) = (50, 50);

    #[test]
    fn g22_row1_silent_press_captures_and_consumes() {
        let mut f = FocusState::new();
        let (mut l, a) = button(0x65, ToggleKind::Flip, 0x05);
        let mut key: u16 = 0x001;
        let r = dispatch_action(l.get_mut(a).unwrap(), 0x01, &mut key, INSIDE, &mut f);
        assert_eq!(r, 1, "press consumed");
        assert_eq!(key, 0, "silent press forces *key = 0");
        assert_eq!(f.sticky, Some(a), "capture acquired");
        let GadgetBehavior::Button(b) = l.get(a).unwrap().behavior else {
            panic!()
        };
        assert!(b.is_pressed);
        assert!(!b.is_on, "press never toggles");
    }

    #[test]
    fn g22_rows2_3_masked0_hover_tracking() {
        let mut f = FocusState::new();
        let (mut l, a) = button(0x65, ToggleKind::Flip, 0x05);
        let mut key: u16 = 0;
        dispatch_action(l.get_mut(a).unwrap(), 0x01, &mut key, INSIDE, &mut f);
        // Drag off: masked-0 re-dispatch with cursor outside pops is_pressed.
        dispatch_action(l.get_mut(a).unwrap(), 0, &mut key, OUTSIDE, &mut f);
        let GadgetBehavior::Button(b) = l.get(a).unwrap().behavior else {
            panic!()
        };
        assert!(!b.is_pressed, "row 2: pop-out");
        // Drag back: pops back in.
        dispatch_action(l.get_mut(a).unwrap(), 0, &mut key, INSIDE, &mut f);
        let GadgetBehavior::Button(b) = l.get(a).unwrap().behavior else {
            panic!()
        };
        assert!(b.is_pressed, "row 3: pop back in");
    }

    #[test]
    fn g22_row5_release_inside_fires_and_toggles_by_kind() {
        for (kind, expect_on_after_1, expect_on_after_2) in [
            (ToggleKind::Flip, true, false),
            (ToggleKind::LatchOn, true, true),
            (ToggleKind::Plain, false, false),
        ] {
            let mut f = FocusState::new();
            let (mut l, a) = button(0xCB, kind, 0x05);
            for (click, expect_on) in [(1, expect_on_after_1), (2, expect_on_after_2)] {
                let mut key: u16 = 0;
                dispatch_action(l.get_mut(a).unwrap(), 0x01, &mut key, INSIDE, &mut f);
                key = 0x801;
                let r = dispatch_action(l.get_mut(a).unwrap(), 0x04, &mut key, INSIDE, &mut f);
                assert_eq!(r, 1);
                assert_eq!(key, 0xCB | 0x8000, "fire on release-inside (click {click})");
                assert_eq!(f.sticky, None, "capture released");
                let GadgetBehavior::Button(b) = l.get(a).unwrap().behavior else {
                    panic!()
                };
                assert!(!b.is_pressed);
                assert_eq!(b.is_on, expect_on, "kind {kind:?} click {click}");
            }
        }
    }

    #[test]
    fn g22_row4_drag_off_cancels() {
        let mut f = FocusState::new();
        let (mut l, a) = button(0x66, ToggleKind::Flip, 0x05);
        let mut key: u16 = 0;
        dispatch_action(l.get_mut(a).unwrap(), 0x01, &mut key, INSIDE, &mut f);
        // Intervening idle tick with the cursor outside pops is_pressed (row 2).
        dispatch_action(l.get_mut(a).unwrap(), 0, &mut key, OUTSIDE, &mut f);
        // Release (sticky re-dispatch): row 4 strips the release bits.
        key = 0x801;
        let r = dispatch_action(l.get_mut(a).unwrap(), 0x04, &mut key, OUTSIDE, &mut f);
        assert_eq!(r, 0, "nothing fires");
        assert_eq!(key, 0x801, "key untouched — no result posted");
        assert_eq!(f.sticky, None, "capture still released by step 3");
        let GadgetBehavior::Button(b) = l.get(a).unwrap().behavior else {
            panic!()
        };
        assert!(!b.is_on, "drag-off cancelled the toggle");
    }

    #[test]
    fn g22_row6_release_outside_no_idle_tick_still_fires() {
        // Boundary case: press then release with NO intervening masked-0 tick.
        let mut f = FocusState::new();
        let (mut l, a) = button(0x65, ToggleKind::Flip, 0x05);
        let mut key: u16 = 0;
        dispatch_action(l.get_mut(a).unwrap(), 0x01, &mut key, INSIDE, &mut f);
        key = 0x801;
        let r = dispatch_action(l.get_mut(a).unwrap(), 0x04, &mut key, OUTSIDE, &mut f);
        assert_eq!(r, 1, "release bits NOT stripped — still fires");
        assert_eq!(key, 0x65 | 0x8000);
        let GadgetBehavior::Button(b) = l.get(a).unwrap().behavior else {
            panic!()
        };
        assert!(!b.is_on, "but no toggle (cursor outside)");
    }

    #[test]
    fn g23_hold_repeat_is_mask_property_only() {
        // Mask WITH a held bit (0x2): every held tick posts the ID again.
        let mut f = FocusState::new();
        let (mut l, a) = button(0x77, ToggleKind::Plain, 0x05 | 0x02);
        let mut key: u16 = 0;
        dispatch_action(l.get_mut(a).unwrap(), 0x01, &mut key, INSIDE, &mut f);
        for _ in 0..3 {
            key = 0;
            let r = dispatch_action(l.get_mut(a).unwrap(), 0x02, &mut key, INSIDE, &mut f);
            assert_eq!(r, 1);
            assert_eq!(key, 0x77 | 0x8000, "held bit repeats the ID every tick");
        }
        // Mask WITHOUT held bits (the 0x55 scroll mask): held ticks mask to 0.
        let mut f2 = FocusState::new();
        let (mut l2, b) = button(0xC9, ToggleKind::Plain, 0x55);
        let mut key2: u16 = 0;
        dispatch_action(l2.get_mut(b).unwrap(), 0x01, &mut key2, INSIDE, &mut f2);
        key2 = 0;
        let masked = 0x02u16 & 0x55; // what clicked_on would mask
        assert_eq!(
            masked, 0,
            "0x55 has no held bits ⇒ masked-0 re-dispatch only"
        );
        let r = dispatch_action(l2.get_mut(b).unwrap(), masked, &mut key2, INSIDE, &mut f2);
        assert_eq!(r, 0);
        assert_eq!(key2, 0, "no repeat for the scroll mask");
    }
}
