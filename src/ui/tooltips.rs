//! Shared tooltip service model (study S1): the native tooltip-manager
//! equivalent consumed by BOTH the in-game sidebar and the front-end shells.
//!
//! Pure and clock-injected: every entry point takes `now_ms: u64`; the app
//! driver (`app::input::tooltips`) is the only place that reads the wall clock.
//! Timer semantics reproduce the native single-timer state machine: one
//! deadline serves both the show delay and the visible-tip duration; polling
//! once per frame lands edges within a frame of the wall-clock deadline —
//! observably identical to the OS-timer pump at any playable frame rate.
//!
//! ## Dependency rules
//! - ui/ module: std only — no render/, assets/, audio/, net/.

/// Delay before a tip shows (native ctor hardcodes 1000 — NOT INI-driven).
pub const TOOLTIP_DELAY_MS: u64 = 1000;
/// Auto-hide duration once shown (native ctor hardcodes 10000).
pub const TOOLTIP_DURATION_MS: u64 = 10_000;
/// Tip text cap (native buffer is 0x100 wide chars).
pub const TOOLTIP_TEXT_CAP_CHARS: usize = 256;

/// Tip rect with INCLUSIVE-both-edges containment — deliberately different
/// from the gadget half-open rule (study S1): the x+w / y+h pixel row/column
/// still hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TipRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl TipRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains_inclusive(&self, px: i32, py: i32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

/// One registered tip region. Text is resolved by the driver at sync time
/// (per frame), which is observably equivalent to the native resolve-at-show
/// (content changes propagate within a frame). EMPTY text reproduces the
/// native "NULL text ⇒ Show fails ⇒ no tip" outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipRegion {
    pub id: u32,
    pub rect: TipRect,
    pub text: String,
}

/// The currently visible tip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveTip {
    pub id: u32,
    pub text: String,
    /// Cursor position captured at show (contract lane §3.3 show snapshot).
    pub x: i32,
    pub y: i32,
    pub shown_at_ms: u64,
}

#[derive(Debug, Default)]
pub struct TooltipService {
    enabled: bool,
    regions: Vec<TipRegion>,
    active: Option<ActiveTip>,
    /// The single timer slot: pending show-delay OR visible-tip duration.
    timer_deadline_ms: Option<u64>,
    /// Cameo-style zero-delay override (contract lane §3.4 hover hook).
    delay_override_ms: Option<u64>,
    /// Opt-in cursor tracking; see [`TooltipService::set_follow_cursor`].
    follow_cursor: bool,
    mouse_x: i32,
    mouse_y: i32,
}

impl TooltipService {
    pub fn new() -> Self {
        Self {
            enabled: true,
            follow_cursor: false,
            ..Default::default()
        }
    }

    fn delay_ms(&self) -> u64 {
        self.delay_override_ms.unwrap_or(TOOLTIP_DELAY_MS)
    }

    /// Register one region; duplicate ids are rejected (native behavior).
    pub fn register(&mut self, region: TipRegion) -> bool {
        if self.regions.iter().any(|r| r.id == region.id) {
            return false;
        }
        self.regions.push(region);
        true
    }

    /// Unregister by id; hides the tip first when it is the visible one.
    pub fn unregister(&mut self, id: u32) -> bool {
        if self.active.as_ref().is_some_and(|a| a.id == id) {
            self.active = None;
        }
        let before = self.regions.len();
        self.regions.retain(|r| r.id != id);
        self.regions.len() != before
    }

    /// Per-frame driver convenience: replace the whole region set (rects move
    /// with the adaptive layout). The visible tip survives iff its id is
    /// still present; a vanished id hides (unregister semantics).
    pub fn sync_regions(&mut self, regions: &[TipRegion]) {
        if let Some(a) = &self.active
            && !regions.iter().any(|r| r.id == a.id)
        {
            self.active = None;
        }
        self.regions.clear();
        self.regions.extend_from_slice(regions);
    }

    /// Mouse move. gamemd never drags a shown tip along with the cursor: the box
    /// keeps the position captured when it appeared, so the 1-px jitter of a
    /// resting hand leaves it in place. Only leaving the region the tip belongs
    /// to, a button press or the duration expiry ends it.
    ///
    /// With [`set_follow_cursor`](Self::set_follow_cursor) enabled every move
    /// hides the tip and re-arms the show delay, which the cameo zero-delay
    /// override re-fires on the same frame's poll — the box then slides after
    /// the cursor. Hiding there rather than leaving it to `poll` is what keeps
    /// that mode from blinking: `poll` reads a firing timer beside a visible tip
    /// as the duration expiry, and that expiry does not re-arm.
    pub fn on_mouse_move(&mut self, x: i32, y: i32, now_ms: u64) {
        if !self.enabled {
            return;
        }
        self.mouse_x = x;
        self.mouse_y = y;
        if !self.follow_cursor
            && self.active.as_ref().is_some_and(|a| {
                self.regions
                    .iter()
                    .any(|r| r.id == a.id && r.rect.contains_inclusive(x, y))
            })
        {
            return;
        }
        self.active = None;
        self.timer_deadline_ms = Some(now_ms + self.delay_ms());
    }

    /// CursorCheat's 724251 -> 72429E branch bypasses the show delay and
    /// refreshes an already visible tip on every move. Button dismissal and
    /// the ordinary duration timer still belong to this same service.
    pub fn on_mouse_move_immediate(&mut self, x: i32, y: i32, now_ms: u64) {
        if !self.enabled {
            return;
        }
        self.mouse_x = x;
        self.mouse_y = y;
        self.active = None;
        self.timer_deadline_ms = Some(now_ms);
        self.poll(now_ms);
    }

    /// Any mouse button press/release (all 6 native button messages,
    /// including middle): kill the timer + hide.
    pub fn on_button(&mut self, _now_ms: u64) {
        self.timer_deadline_ms = None;
        if self.active.is_some() {
            self.active = None;
        }
    }

    /// The timer pump (call once per frame). Native WM_TIMER semantics: a
    /// firing timer with a visible tip hides it (duration expiry) and arms
    /// nothing; otherwise the cursor is hit-tested against the regions in
    /// REGISTRATION order (first match wins, inclusive edges) and a
    /// non-empty-text match shows + re-arms the duration timer.
    pub fn poll(&mut self, now_ms: u64) {
        if !self.enabled {
            return;
        }
        let Some(deadline) = self.timer_deadline_ms else {
            return;
        };
        if now_ms < deadline {
            return;
        }
        self.timer_deadline_ms = None;
        if self.active.is_some() {
            self.active = None;
            return;
        }
        let hit = self
            .regions
            .iter()
            .find(|r| r.rect.contains_inclusive(self.mouse_x, self.mouse_y));
        if let Some(r) = hit
            && !r.text.is_empty()
        {
            let text: String = r.text.chars().take(TOOLTIP_TEXT_CAP_CHARS).collect();
            self.active = Some(ActiveTip {
                id: r.id,
                text,
                x: self.mouse_x,
                y: self.mouse_y,
                shown_at_ms: now_ms,
            });
            self.timer_deadline_ms = Some(now_ms + TOOLTIP_DURATION_MS);
        }
    }

    pub fn active(&self) -> Option<&ActiveTip> {
        self.active.as_ref()
    }

    /// Cameo-hover hook (slice A2 consumer; exposed + tested now): zero delay
    /// while highlighted, restored on leave.
    pub fn set_delay_override(&mut self, delay_ms: Option<u64>) {
        self.delay_override_ms = delay_ms;
    }

    /// Whether an already visible tip follows the cursor.
    ///
    /// Off by default, which is gamemd's behaviour: the pop-up holds the spot it
    /// appeared in. Native's one tracking branch is the `CursorCheat`
    /// 724251 → 72429E path behind the in-game coordinate toggle, served by
    /// [`Self::on_mouse_move_immediate`] and independent of this flag; turning
    /// it on here makes the ordinary path re-show the tip at the new cursor
    /// position on every move instead.
    pub fn set_follow_cursor(&mut self, follow_cursor: bool) {
        self.follow_cursor = follow_cursor;
    }

    /// Enable gate; disabling kills the timer and hides immediately.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.timer_deadline_ms = None;
            self.active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(id: u32, x: i32, y: i32, w: i32, h: i32, text: &str) -> TipRegion {
        TipRegion {
            id,
            rect: TipRect::new(x, y, w, h),
            text: text.to_string(),
        }
    }

    fn service_with(regions: &[TipRegion]) -> TooltipService {
        let mut s = TooltipService::new();
        for r in regions {
            assert!(s.register(r.clone()));
        }
        s
    }

    #[test]
    fn shows_after_exactly_1000_ms_of_stillness() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.on_mouse_move(5, 5, 0);
        s.poll(999);
        assert!(s.active().is_none(), "1 ms early: nothing");
        s.poll(1000);
        let tip = s.active().expect("shown at the deadline");
        assert_eq!(tip.id, 1);
        assert_eq!(tip.shown_at_ms, 1000);
        assert_eq!((tip.x, tip.y), (5, 5), "cursor captured at show");
    }

    #[test]
    fn coordinates_refresh_visible_tip_and_restore_ordinary_delay() {
        let mut s = service_with(&[region(500, 0, 0, 100, 100, "(10,20)")]);
        s.on_mouse_move_immediate(5, 6, 100);
        assert_eq!(s.active().unwrap().text, "(10,20)");
        s.sync_regions(&[region(500, 0, 0, 100, 100, "(11,20)")]);
        s.on_mouse_move_immediate(7, 6, 200);
        let tip = s.active().unwrap();
        assert_eq!((&*tip.text, tip.x, tip.shown_at_ms), ("(11,20)", 7, 200));
        s.poll(10_199);
        assert!(s.active().is_some());
        s.poll(10_200);
        assert!(s.active().is_none());
        s.on_mouse_move_immediate(8, 6, 11_000);
        s.on_button(11_001);
        s.poll(30_000);
        assert!(s.active().is_none());
        s.on_mouse_move(9, 6, 31_000);
        s.poll(31_999);
        assert!(s.active().is_none());
        s.poll(32_000);
        assert!(s.active().is_some());
        s.set_enabled(false);
        s.on_mouse_move_immediate(10, 6, 33_000);
        assert!(s.active().is_none());
    }

    #[test]
    fn every_move_restarts_the_delay_and_hides() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.set_follow_cursor(true);
        s.on_mouse_move(5, 5, 0);
        s.on_mouse_move(6, 5, 900);
        s.poll(1000);
        assert!(s.active().is_none(), "restarted at 900 → deadline 1900");
        s.poll(1900);
        assert!(s.active().is_some());
        // A move while visible hides AND re-arms.
        s.on_mouse_move(7, 5, 2000);
        assert!(s.active().is_none(), "moving hides a visible tip");
        s.poll(3000);
        assert!(s.active().is_some(), "re-shown after another full delay");
    }

    /// gamemd holds a shown tip in the spot it appeared in: moving inside its
    /// region neither moves nor hides it, and only leaving the region ends it.
    #[test]
    fn shown_tip_holds_its_spot_until_the_cursor_leaves() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.set_delay_override(Some(0));
        s.on_mouse_move(2, 2, 1000);
        s.poll(1000);
        assert_eq!((s.active().unwrap().x, s.active().unwrap().y), (2, 2));
        for (x, y) in [(4, 3), (7, 8), (9, 1)] {
            s.on_mouse_move(x, y, 1200);
            s.poll(1200);
            let tip = s.active().expect("still inside the region");
            assert_eq!((tip.x, tip.y), (2, 2), "tip slid after the cursor");
            assert_eq!(tip.shown_at_ms, 1000, "the duration timer restarted");
        }
        s.on_mouse_move(20, 20, 1400);
        assert!(s.active().is_none(), "leaving the region hides");
        s.poll(1400);
        assert!(s.active().is_none(), "nothing to hit outside the region");
    }

    #[test]
    fn duration_auto_hide_re_arm() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert!(s.active().is_some());
        s.poll(10_999);
        assert!(s.active().is_some(), "duration is 10000 ms from show");
        s.poll(11_000);
        assert!(s.active().is_none(), "auto-hide at shown+10000");
        s.poll(50_000);
        assert!(s.active().is_none(), "stays hidden until the next move");
    }

    #[test]
    fn any_button_kills_timer_and_tip() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.on_mouse_move(5, 5, 0);
        s.on_button(500);
        s.poll(1000);
        assert!(s.active().is_none(), "pending timer killed");
        s.on_mouse_move(5, 5, 2000);
        s.poll(3000);
        assert!(s.active().is_some());
        s.on_button(3100);
        assert!(s.active().is_none(), "visible tip killed");
    }

    #[test]
    fn inclusive_both_edges_vs_gadget_half_open() {
        // Region {x:10, y:10, w:5, h:5}: the (15,15) corner pixel HITS the
        // tip rect (inclusive) but MISSES a gadget rect of the same numbers
        // (half-open) — the deliberate 1-px observable difference (study S1).
        let mut s = service_with(&[region(1, 10, 10, 5, 5, "tip")]);
        s.on_mouse_move(15, 15, 0);
        s.poll(1000);
        assert!(s.active().is_some(), "inclusive edge hits");
        let gadget_rect = crate::ui::gadget::GadgetRect::new(10, 10, 5, 5);
        assert!(
            !gadget_rect.contains(15, 15),
            "same point misses the gadget rect"
        );
        // One past the inclusive edge misses.
        let mut s2 = service_with(&[region(1, 10, 10, 5, 5, "tip")]);
        s2.on_mouse_move(16, 15, 0);
        s2.poll(1000);
        assert!(s2.active().is_none());
    }

    #[test]
    fn first_registered_wins_on_overlap() {
        let mut s = service_with(&[
            region(1, 0, 0, 100, 100, "first"),
            region(2, 0, 0, 10, 10, "second-smaller"),
        ]);
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert_eq!(
            s.active().unwrap().text,
            "first",
            "registration order, NOT smallest-area (unlike the gadget rule)"
        );
    }

    #[test]
    fn duplicate_register_rejected_unregister_hides() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        assert!(
            !s.register(region(1, 50, 50, 5, 5, "dup")),
            "dup id rejected"
        );
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert!(s.active().is_some());
        assert!(s.unregister(1));
        assert!(s.active().is_none(), "unregister hides the visible tip");
        assert!(!s.unregister(1));
    }

    #[test]
    fn empty_text_never_shows() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "")]);
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert!(
            s.active().is_none(),
            "NULL/empty text ⇒ Show fails ⇒ no tip"
        );
    }

    #[test]
    fn zero_delay_override_shows_on_next_poll() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "cameo")]);
        s.set_delay_override(Some(0));
        s.on_mouse_move(5, 5, 7000);
        s.poll(7000);
        assert!(s.active().is_some(), "cameo-hover zero delay: immediate");
        // A frozen tip only ends when the cursor leaves its region, so the
        // restored delay is measured from that next hiding move.
        s.set_delay_override(None);
        s.on_mouse_move(50, 50, 7100);
        assert!(s.active().is_none(), "leaving the region hides");
        s.on_mouse_move(6, 5, 7150);
        s.poll(7500);
        assert!(s.active().is_none(), "restored 1000 ms delay applies again");
        s.poll(8150);
        assert!(s.active().is_some());
    }

    #[test]
    fn zero_delay_jitter_does_not_toggle_the_visible_tip() {
        // A mouse resting on a cameo still emits 1-px jitter, and the cameo
        // hover hook keeps the show delay at zero. The jitter must neither blink
        // the tip nor move it: hiding on a move is read by `poll` as the
        // duration expiry, which never re-arms, so the tip blinks at the event
        // rate.
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "cameo")]);
        s.set_delay_override(Some(0));
        s.on_mouse_move(5, 5, 1000);
        s.poll(1000);
        assert!(s.active().is_some());
        for now in [1016, 1032, 1048, 1064] {
            s.on_mouse_move(5, 6, now);
            s.poll(now);
            let tip = s
                .active()
                .unwrap_or_else(|| panic!("tip blinked at {now} ms"));
            assert_eq!((tip.x, tip.y), (5, 5), "jitter moved the tip at {now} ms");
        }
    }

    #[test]
    fn sync_regions_keeps_active_iff_id_survives() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert!(s.active().is_some());
        // Same id, moved rect: tip survives.
        s.sync_regions(&[region(1, 100, 100, 10, 10, "tip")]);
        assert!(s.active().is_some());
        // Id gone: hides.
        s.sync_regions(&[region(2, 0, 0, 10, 10, "other")]);
        assert!(s.active().is_none());
    }

    #[test]
    fn disable_kills_and_gates() {
        let mut s = service_with(&[region(1, 0, 0, 10, 10, "tip")]);
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert!(s.active().is_some());
        s.set_enabled(false);
        assert!(s.active().is_none());
        s.on_mouse_move(5, 5, 2000);
        s.poll(5000);
        assert!(s.active().is_none(), "disabled service ignores input");
    }

    #[test]
    fn text_capped_at_256_chars() {
        let long = "x".repeat(400);
        let mut s = service_with(&[region(1, 0, 0, 10, 10, &long)]);
        s.on_mouse_move(5, 5, 0);
        s.poll(1000);
        assert_eq!(s.active().unwrap().text.chars().count(), 256);
    }
}
